//! Publishing use case: batch preconditions, per-sample evaluation, sequence CAS, snapshot
//! and the per-connection token bucket (docs/08 §6.4 and §7, docs/09 §3–§4, ADR-0008,
//! FR-LOC-001..008, FR-STR-004).
//!
//! The preconditions are evaluated in the order of `docs/08` §6.4; the first that fails
//! determines `request_error` and no sample is applied. When they pass, each sample follows
//! the algorithm of `docs/09` §4: pure validation, age check and atomic sequence CAS on
//! `{p}:seq:{session}:{device}`. Every accepted sample produces one `LocationUpdate` with the
//! server clock as `received_at` (FR-LOC-005), refreshes the snapshot of the member when the
//! session allows it and lifts `{p}:devseq:{device}` (FR-LOC-008).
//!
//! Delivery to other subscribers is the fan-out of T-F4-06; this module returns the accepted
//! updates so the connection can at least deliver them to its own subscription (docs/08 §7.3).
#![forbid(unsafe_code)]

use gs_domain::error::{AsErrorCode, ErrorCode};
use gs_domain::ids::{DeviceId, MemberId, SessionId, SubjectId, TenantId};
use gs_domain::location::{
    LocationPolicy, RawLocationSample, RawPosition, RawQuality, SampleResult, validate_sample,
};
use gs_ephemeral::{EphemeralStore, SequenceCas};
use gs_protocol::{
    DeliveryKind, ErrorCode as ProtoErrorCode, LocationSample as ProtoSample, LocationUpdate,
    PublishAck, SampleResult as ProtoSampleResult, SampleStatus as ProtoSampleStatus,
};
use gs_store::repos::{MembershipRepository, SessionRepository};
use gs_store::{PgPool, StoreError};
use pbjson_types::Timestamp;
use prost::Message as _;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use time::OffsetDateTime;

/// Token bucket of one (connection, session) pair (docs/08 §7.4).
#[derive(Debug, Clone)]
pub struct PublishThrottle {
    capacity: u32,
    /// Interval between tokens, in milliseconds.
    interval_ms: u32,
    tokens: u32,
    refilled_at: OffsetDateTime,
}

impl PublishThrottle {
    /// Builds a bucket with `publish_burst` tokens refilled every `min_publish_interval_ms`.
    pub fn new(burst: u32, interval: Duration, now: OffsetDateTime) -> Self {
        Self {
            capacity: burst.max(1),
            interval_ms: u32::try_from(interval.as_millis().min(u128::from(u32::MAX)))
                .unwrap_or(1)
                .max(1),
            tokens: burst.max(1),
            refilled_at: now,
        }
    }

    /// Tries to take one token; a `PublishLocation` costs one regardless of the sample count.
    pub fn try_acquire(&mut self, now: OffsetDateTime) -> bool {
        self.refill(now);
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }

    /// Milliseconds until the next token: the `retry_after_ms` of a 429-equivalent answer.
    pub fn retry_after_ms(&self, now: OffsetDateTime) -> u32 {
        let elapsed = (now - self.refilled_at).max(time::Duration::ZERO);
        let interval_ms = i128::from(self.interval_ms);
        let elapsed_ms = elapsed.whole_milliseconds();
        let remaining = (interval_ms - elapsed_ms).max(1);
        u32::try_from(remaining).unwrap_or(u32::MAX)
    }

    /// Refills the bucket for the elapsed time.
    fn refill(&mut self, now: OffsetDateTime) {
        let elapsed = (now - self.refilled_at).max(time::Duration::ZERO);
        let interval_ms = i128::from(self.interval_ms);
        let periods = elapsed.whole_milliseconds() / interval_ms.max(1);
        if periods > 0 {
            let gained = u32::try_from(periods).unwrap_or(u32::MAX);
            self.tokens = self.tokens.saturating_add(gained).min(self.capacity);
            self.refilled_at = now;
        }
    }
}

/// Principal publishing: only devices may publish (docs/06 §8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishCaller {
    /// Subject of the device.
    pub subject_id: SubjectId,
    /// Device of the connection.
    pub device_id: DeviceId,
}

/// Everything a `PublishLocation` needs.
pub struct PublishParams<'a, E: ?Sized> {
    /// PostgreSQL pool.
    pub pool: &'a PgPool,
    /// Ephemeral store: sequence CAS, snapshots and the device sequence floor.
    pub ephemeral: &'a E,
    /// Ephemeral key prefix (`GS_REDIS_KEY_PREFIX`, default `gs`).
    pub key_prefix: &'a str,
    /// Tenant of the principal.
    pub tenant_id: TenantId,
    /// Session the samples belong to.
    pub session_id: SessionId,
    /// Publishing principal.
    pub caller: PublishCaller,
    /// Samples of the batch, in the order received.
    pub samples: &'a [ProtoSample],
    /// Tenant policy values that shape the limits.
    pub policy: LocationPolicy,
    /// Token bucket of (connection, session).
    pub throttle: &'a mut PublishThrottle,
    /// Current instant, the server clock.
    pub now: OffsetDateTime,
}

/// Errors of publication, mapped to the catalog of `docs/09` §6.
#[derive(Debug, Error)]
pub enum PublishError {
    /// The batch is empty, its sequences are not strictly increasing, or the session id is
    /// malformed (docs/08 §6.4).
    #[error("invalid publish request")]
    ValidationFailed,

    /// The batch has more samples than the policy allows.
    #[error("too many samples in one publish")]
    PayloadTooLarge,

    /// An application principal may not publish (docs/06 §8.3).
    #[error("application principals cannot publish")]
    PermissionDenied(&'static str),

    /// The token bucket of (connection, session) is empty.
    #[error("publish rate limit exceeded")]
    RateLimited {
        /// Milliseconds until the next token.
        retry_after_ms: u32,
    },

    /// The subject has no active membership in the session (FR-API-006).
    #[error("the subject is not an active member of the session")]
    NotFound,

    /// The membership of the subject was revoked.
    #[error("the membership was revoked")]
    MemberRevoked,

    /// The membership lacks the `publish` capability.
    #[error("the membership cannot publish")]
    NotPublisher,

    /// The session is closed (INV-SES-1).
    #[error("session is closed")]
    SessionClosed,

    /// The session is active but expired (INV-SES-2).
    #[error("session is expired")]
    SessionExpired,

    /// The subject is not sharing its location (FR-SHR-002).
    #[error("the subject is not sharing")]
    NotSharing,

    /// Another device publishes for that membership (FR-SHR-004).
    #[error("this device is not the publisher")]
    NotPublisherDevice,

    /// The ephemeral store is degraded or unreachable (`docs/04` §7.6).
    #[error("publish store temporarily unavailable")]
    TemporarilyUnavailable {
        /// Milliseconds the client should wait before retrying.
        retry_after_ms: u32,
    },

    /// A persistence operation failed.
    #[error("storage error: {0}")]
    Store(#[from] StoreError),
}

impl AsErrorCode for PublishError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::ValidationFailed => ErrorCode::ValidationFailed,
            Self::PayloadTooLarge => ErrorCode::PayloadTooLarge,
            Self::PermissionDenied(_) => ErrorCode::PermissionDenied,
            Self::RateLimited { .. } => ErrorCode::RateLimited,
            Self::NotFound => ErrorCode::NotFound,
            Self::MemberRevoked => ErrorCode::MemberRevoked,
            Self::NotPublisher => ErrorCode::PermissionDenied,
            Self::SessionClosed => ErrorCode::SessionClosed,
            Self::SessionExpired => ErrorCode::SessionExpired,
            Self::NotSharing => ErrorCode::NotSharing,
            Self::NotPublisherDevice => ErrorCode::NotPublisher,
            Self::TemporarilyUnavailable { .. } => ErrorCode::TemporarilyUnavailable,
            Self::Store(_) => ErrorCode::Internal,
        }
    }
}

/// Result of a `PublishLocation`.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishOutcome {
    /// One result per sample, in the same order.
    pub results: Vec<SampleResult>,
    /// Updates accepted, to deliver (docs/08 §7.2).
    pub accepted: Vec<LocationUpdate>,
    /// Highest accepted sequence of the batch, when at least one sample was accepted.
    pub last_sequence: Option<u64>,
}

/// Publishes a batch of samples (docs/08 §6.4, docs/09 §4).
pub async fn publish_batch<E: EphemeralStore + ?Sized>(
    params: PublishParams<'_, E>,
) -> Result<PublishOutcome, PublishError> {
    let PublishParams {
        pool,
        ephemeral,
        key_prefix,
        tenant_id,
        session_id,
        caller,
        samples,
        policy,
        throttle,
        now,
    } = params;

    // 1. Batch preconditions (docs/08 §6.4), in the documented order.
    if samples.is_empty() {
        return Err(PublishError::ValidationFailed);
    }
    if !strictly_increasing(samples) {
        return Err(PublishError::ValidationFailed);
    }
    if samples.len() > policy.max_samples_per_publish as usize {
        return Err(PublishError::PayloadTooLarge);
    }
    if !throttle.try_acquire(now) {
        let retry_after_ms = throttle.retry_after_ms(now);
        return Err(PublishError::RateLimited { retry_after_ms });
    }

    let session = SessionRepository::get_by_id(pool, tenant_id, session_id)
        .await?
        .ok_or(PublishError::NotFound)?;
    match session.status {
        gs_domain::session::SessionStatus::Active if session.expires_at <= now => {
            return Err(PublishError::SessionExpired);
        }
        gs_domain::session::SessionStatus::Active => {}
        gs_domain::session::SessionStatus::Closed => return Err(PublishError::SessionClosed),
        gs_domain::session::SessionStatus::Expired => return Err(PublishError::SessionExpired),
    }

    let membership = MembershipRepository::find_latest_by_session_and_subject(
        pool,
        tenant_id,
        session_id,
        caller.subject_id,
    )
    .await?;
    // `find_latest_by_session_and_subject` distinguishes "never was a member" from "was
    // revoked", which the table of docs/08 §6.4 answers differently.
    let Some(membership) = membership else {
        return Err(PublishError::NotFound);
    };
    if membership.status == gs_domain::membership::MemberStatusKind::Revoked {
        return Err(PublishError::MemberRevoked);
    }
    if membership.status != gs_domain::membership::MemberStatusKind::Active {
        return Err(PublishError::NotFound);
    }
    if !membership.capabilities.publish {
        return Err(PublishError::NotPublisher);
    }
    if !membership.sharing {
        return Err(PublishError::NotSharing);
    }
    if membership.publisher_device_id != Some(caller.device_id) {
        return Err(PublishError::NotPublisherDevice);
    }

    // 2. Per-sample evaluation (docs/09 §4): validation, age and atomic sequence CAS.
    let seq_key = format!(
        "{key_prefix}:seq:{session_id}:{device}",
        device = caller.device_id
    );
    let mut results = Vec::with_capacity(samples.len());
    let mut accepted = Vec::new();
    let mut last_sequence = None;

    for sample in samples {
        let raw = raw_sample(sample);
        let validated = match validate_sample(&raw, &policy, now) {
            Ok(validated) => validated,
            Err(error) => {
                results.push(SampleResult::rejected(
                    sample.sequence,
                    error.error_code,
                    error.field,
                ));
                continue;
            }
        };

        // 3. Age check (docs/09 §3.2): older than `max_event_age_ms` is stale, not invalid.
        if validated.captured_at + time::Duration::milliseconds(policy.max_event_age_ms) < now {
            results.push(SampleResult::ignored_stale(validated.sequence));
            continue;
        }

        // 4. Sequence CAS on the ephemeral store (docs/09 §4, ADR-0008).
        //
        // The caller never reads the stored value first: the store compares atomically and
        // `current = 0` means "no expectation", which is exactly the semantics of docs/09 §4.
        match ephemeral
            .compare_and_set_sequence(&seq_key, 0, validated.sequence)
            .await
        {
            Ok(SequenceCas::Duplicate { .. }) => {
                results.push(SampleResult::ignored_duplicate(validated.sequence));
                continue;
            }
            Ok(SequenceCas::Stale { .. }) => {
                results.push(SampleResult::ignored_stale(validated.sequence));
                continue;
            }
            Ok(SequenceCas::Accepted { .. }) => {}
            Err(_) => {
                return Err(PublishError::TemporarilyUnavailable {
                    retry_after_ms: 1_000,
                });
            }
        }

        let update = LocationUpdate {
            session_id: session_id.to_string(),
            member_id: membership.id.to_string(),
            sample: Some(proto_sample(sample)),
            received_at: Some(to_proto_timestamp(now)),
            delivery: DeliveryKind::Live as i32,
        };
        results.push(SampleResult::accepted(validated.sequence));
        accepted.push(update);
        last_sequence = Some(validated.sequence);
    }

    // 5. Effects of the accepted samples (docs/09 §4).
    if let Some(update) = accepted.last() {
        if session.snapshot_ttl_seconds > 0 {
            let key = snapshot_key(key_prefix, session_id, membership.id);
            let ttl = Duration::from_secs(u64::from(session.snapshot_ttl_seconds));
            // A snapshot that cannot be written is not fatal: it expires by its own TTL and the
            // next accepted sample retries (docs/11 §2.1).
            let _ = ephemeral
                .set_snapshot(&key, &update.encode_to_vec(), ttl)
                .await;
        }
        let device_key = format!("{key_prefix}:devseq:{device}", device = caller.device_id);
        let floor = last_sequence.unwrap_or_default();
        let _ = ephemeral.set_if_greater(&device_key, floor).await;
    }

    Ok(PublishOutcome {
        results,
        accepted,
        last_sequence,
    })
}

/// Builds the `PublishAck` of an outcome: one result per sample, same order (docs/08 §6.4).
pub fn publish_ack_for(session_id: &str, outcome: &PublishOutcome) -> PublishAck {
    PublishAck {
        session_id: session_id.to_string(),
        // The batch was processed: per-sample results carry the outcome.
        request_error: ProtoErrorCode::Unspecified as i32,
        results: outcome.results.iter().map(proto_sample_result).collect(),
        retry_after_ms: 0,
    }
}

/// Converts a domain result into the wire result (docs/08 §6.4).
fn proto_sample_result(result: &SampleResult) -> ProtoSampleResult {
    let (status, error_code, field) = match &result.status {
        gs_domain::location::SampleStatus::Accepted => (ProtoSampleStatus::Accepted, None, None),
        gs_domain::location::SampleStatus::IgnoredDuplicate => {
            (ProtoSampleStatus::IgnoredDuplicate, None, None)
        }
        gs_domain::location::SampleStatus::IgnoredStale => {
            (ProtoSampleStatus::IgnoredStale, None, None)
        }
        gs_domain::location::SampleStatus::Rejected => (
            ProtoSampleStatus::Rejected,
            result
                .error_code
                .as_deref()
                // The proto names carry the `ERROR_CODE_` prefix; the catalog of docs/09 §6
                // does not (docs/08 §12).
                .and_then(|name| ProtoErrorCode::from_str_name(&format!("ERROR_CODE_{name}")))
                .map(|code| code as i32),
            result.field.clone(),
        ),
    };
    ProtoSampleResult {
        sequence: result.sequence,
        status: status as i32,
        error_code: error_code.unwrap_or(ProtoErrorCode::Unspecified as i32),
        field: field.unwrap_or_default(),
    }
}

/// Key of the sequence CAS of a (session, device) pair (docs/11 §3).
pub fn sequence_key(key_prefix: &str, session_id: SessionId, device_id: DeviceId) -> String {
    format!("{key_prefix}:seq:{session_id}:{device_id}")
}

/// Key of the snapshot of a member (docs/11 §3).
pub fn snapshot_key(key_prefix: &str, session_id: SessionId, member_id: MemberId) -> String {
    format!("{key_prefix}:snap:{session_id}:{member_id}")
}

/// Key of the device sequence floor (docs/11 §3).
pub fn device_sequence_key(key_prefix: &str, device_id: DeviceId) -> String {
    format!("{key_prefix}:devseq:{device_id}")
}

/// Whether the sequences of the batch are strictly increasing (docs/08 §6.4).
fn strictly_increasing(samples: &[ProtoSample]) -> bool {
    samples
        .windows(2)
        .all(|pair| pair[1].sequence > pair[0].sequence)
}

/// Converts a wire sample into the raw domain sample the validator consumes.
fn raw_sample(sample: &ProtoSample) -> RawLocationSample {
    RawLocationSample {
        sequence: Some(sample.sequence),
        sequence_invalid: false,
        captured_at: sample.captured_at.as_ref().and_then(from_proto_timestamp),
        captured_at_invalid: false,
        position: sample.position.as_ref().map(|position| RawPosition {
            latitude: Some(position.latitude),
            longitude: Some(position.longitude),
            horizontal_accuracy_m: Some(position.horizontal_accuracy_m),
            altitude_m: position.altitude_m,
            vertical_accuracy_m: position.vertical_accuracy_m,
            speed_mps: position.speed_mps,
            speed_accuracy_mps: position.speed_accuracy_mps,
            heading_deg: position.heading_deg,
            heading_accuracy_deg: position.heading_accuracy_deg,
        }),
        quality: sample.quality.as_ref().map(|quality| RawQuality {
            source: Some(domain_source(quality.source)),
            kind: Some(domain_kind(quality.kind)),
            is_mock: quality.is_mock,
            motion: Some(domain_motion(quality.motion)),
        }),
    }
}

/// Rebuilds the wire sample that will be delivered (docs/08 §7.2: never modified).
fn proto_sample(sample: &ProtoSample) -> ProtoSample {
    *sample
}

/// Wire location source to the domain enum, by name (docs/08 §12).
fn domain_source(source: i32) -> gs_domain::location::LocationSource {
    let name = match gs_protocol::LocationSource::try_from(source) {
        Ok(gs_protocol::LocationSource::Gnss) => "GNSS",
        Ok(gs_protocol::LocationSource::Network) => "NETWORK",
        Ok(gs_protocol::LocationSource::Fused) => "FUSED",
        Ok(gs_protocol::LocationSource::Manual) => "MANUAL",
        Ok(gs_protocol::LocationSource::ExternalDevice) => "EXTERNAL_DEVICE",
        // UNSPECIFIED and unknown values are accepted (docs/09 §3.1).
        _ => "UNSPECIFIED",
    };
    gs_domain::location::LocationSource::from_str(name).unwrap_or_default()
}

/// Wire location kind to the domain enum, by name (docs/08 §12).
fn domain_kind(kind: i32) -> gs_domain::location::LocationKind {
    let name = match gs_protocol::LocationKind::try_from(kind) {
        Ok(gs_protocol::LocationKind::Measured) => "MEASURED",
        Ok(gs_protocol::LocationKind::Filtered) => "FILTERED",
        Ok(gs_protocol::LocationKind::Interpolated) => "INTERPOLATED",
        Ok(gs_protocol::LocationKind::Predicted) => "PREDICTED",
        Ok(gs_protocol::LocationKind::MapMatched) => "MAP_MATCHED",
        // UNSPECIFIED is invalid: the validator rejects it (docs/09 §3.1).
        _ => "UNSPECIFIED",
    };
    gs_domain::location::LocationKind::from_str(name).unwrap_or_default()
}

/// Wire motion activity to the domain enum, by name (docs/08 §12).
fn domain_motion(motion: i32) -> gs_domain::location::MotionActivity {
    let name = match gs_protocol::MotionActivity::try_from(motion) {
        Ok(gs_protocol::MotionActivity::Stationary) => "STATIONARY",
        Ok(gs_protocol::MotionActivity::Walking) => "WALKING",
        Ok(gs_protocol::MotionActivity::Running) => "RUNNING",
        Ok(gs_protocol::MotionActivity::Cycling) => "CYCLING",
        Ok(gs_protocol::MotionActivity::Automotive) => "AUTOMOTIVE",
        _ => "UNSPECIFIED",
    };
    gs_domain::location::MotionActivity::from_str(name).unwrap_or_default()
}

/// Converts a protobuf timestamp into an instant.
fn from_proto_timestamp(timestamp: &Timestamp) -> Option<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(timestamp.seconds)
        .ok()
        .map(|instant| instant + time::Duration::nanoseconds(i64::from(timestamp.nanos)))
}

/// Converts an instant into a protobuf timestamp.
fn to_proto_timestamp(instant: OffsetDateTime) -> Timestamp {
    Timestamp {
        seconds: instant.unix_timestamp(),
        nanos: instant.nanosecond() as i32,
    }
}
