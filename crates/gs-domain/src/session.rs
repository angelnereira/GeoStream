//! Session domain model, lifecycle state machine and invariants (docs/05 §4).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use time::OffsetDateTime;

use crate::ids::{ApplicationId, DeviceId, SessionId, SubjectId, TenantId};

/// Session operating modes (docs/05 §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMode {
    #[serde(rename = "ONE_TO_ONE", alias = "SESSION_MODE_ONE_TO_ONE")]
    OneToOne,
    #[serde(rename = "ONE_TO_MANY", alias = "SESSION_MODE_ONE_TO_MANY")]
    OneToMany,
    #[serde(rename = "MANY_TO_MANY", alias = "SESSION_MODE_MANY_TO_MANY")]
    ManyToMany,
}

impl fmt::Display for SessionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OneToOne => write!(f, "ONE_TO_ONE"),
            Self::OneToMany => write!(f, "ONE_TO_MANY"),
            Self::ManyToMany => write!(f, "MANY_TO_MANY"),
        }
    }
}

impl FromStr for SessionMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ONE_TO_ONE" | "SESSION_MODE_ONE_TO_ONE" => Ok(Self::OneToOne),
            "ONE_TO_MANY" | "SESSION_MODE_ONE_TO_MANY" => Ok(Self::OneToMany),
            "MANY_TO_MANY" | "SESSION_MODE_MANY_TO_MANY" => Ok(Self::ManyToMany),
            _ => Err(()),
        }
    }
}

/// Lifecycle status of a session (docs/05 §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionStatus {
    #[serde(rename = "ACTIVE", alias = "SESSION_STATUS_ACTIVE")]
    Active,
    #[serde(rename = "CLOSED", alias = "SESSION_STATUS_CLOSED")]
    Closed,
    #[serde(rename = "EXPIRED", alias = "SESSION_STATUS_EXPIRED")]
    Expired,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "ACTIVE"),
            Self::Closed => write!(f, "CLOSED"),
            Self::Expired => write!(f, "EXPIRED"),
        }
    }
}

impl FromStr for SessionStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ACTIVE" | "SESSION_STATUS_ACTIVE" => Ok(Self::Active),
            "CLOSED" | "SESSION_STATUS_CLOSED" => Ok(Self::Closed),
            "EXPIRED" | "SESSION_STATUS_EXPIRED" => Ok(Self::Expired),
            _ => Err(()),
        }
    }
}

/// Reason why a session was closed (docs/05 §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ClosedReason {
    #[serde(rename = "CLOSED_BY_MEMBER")]
    ClosedByMember,
    #[serde(rename = "CLOSED_BY_APPLICATION")]
    ClosedByApplication,
    #[serde(rename = "OWNER_LEFT")]
    OwnerLeft,
    #[serde(rename = "ADMIN")]
    Admin,
}

impl fmt::Display for ClosedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClosedByMember => write!(f, "CLOSED_BY_MEMBER"),
            Self::ClosedByApplication => write!(f, "CLOSED_BY_APPLICATION"),
            Self::OwnerLeft => write!(f, "OWNER_LEFT"),
            Self::Admin => write!(f, "ADMIN"),
        }
    }
}

/// Actor that created a session or triggered a domain action.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Actor {
    Device(DeviceId),
    Application(ApplicationId),
}

/// Session domain error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("session is not active")]
    InactiveSession,
    #[error("session is closed")]
    SessionClosed,
    #[error("session has expired")]
    SessionExpired,
    #[error("session is full (reached max_members)")]
    SessionFull,
    #[error("cannot reduce member limit below active members count")]
    MaxMembersLessThanActive,
    #[error("invalid member limit: {0}")]
    InvalidMaxMembers(String),
    #[error("invalid expiration: {0}")]
    InvalidExpiration(String),
    #[error("invalid metadata: {0}")]
    InvalidMetadata(String),
}

/// A tracking session (docs/05 §4).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub tenant_id: TenantId,
    pub mode: SessionMode,
    pub status: SessionStatus,
    pub owner_subject_id: Option<SubjectId>,
    pub created_by: Actor,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
    pub max_members: u32,
    pub snapshot_ttl_seconds: u32,
    pub stale_after_seconds: u32,
    pub metadata: BTreeMap<String, String>,
    pub closed_reason: Option<ClosedReason>,
    pub version: u64,
}

impl Session {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: SessionId,
        tenant_id: TenantId,
        mode: SessionMode,
        owner_subject_id: Option<SubjectId>,
        created_by: Actor,
        created_at: OffsetDateTime,
        expires_at: OffsetDateTime,
        max_members: u32,
        snapshot_ttl_seconds: u32,
        stale_after_seconds: u32,
        metadata: BTreeMap<String, String>,
        max_session_ttl_seconds: u32,
    ) -> Result<Self, SessionError> {
        // Mode member bounds (docs/05 §4.2)
        if mode == SessionMode::OneToOne && max_members != 2 {
            return Err(SessionError::InvalidMaxMembers(
                "ONE_TO_ONE mode requires max_members == 2".to_string(),
            ));
        }
        if max_members < 2 {
            return Err(SessionError::InvalidMaxMembers(
                "max_members must be >= 2".to_string(),
            ));
        }

        // TTL check: expires_at <= created_at + max_session_ttl_seconds
        let max_ttl = time::Duration::seconds(max_session_ttl_seconds as i64);
        let max_allowed = match created_at.checked_add(max_ttl) {
            Some(t) => t,
            None => return Err(SessionError::InvalidExpiration("TTL overflow".to_string())),
        };
        if expires_at <= created_at || expires_at > max_allowed {
            return Err(SessionError::InvalidExpiration(
                "expires_at exceeds allowed session TTL".to_string(),
            ));
        }

        // Metadata validation (docs/09 §5: <= 32 entries, key ^[a-z0-9_.-]{1,64}$, val <= 512)
        if metadata.len() > 32 {
            return Err(SessionError::InvalidMetadata(
                "metadata entries exceed 32".to_string(),
            ));
        }
        for (k, v) in &metadata {
            if k.is_empty()
                || k.len() > 64
                || !k.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-')
                })
            {
                return Err(SessionError::InvalidMetadata(format!(
                    "invalid metadata key format: {k}"
                )));
            }
            if v.len() > 512 {
                return Err(SessionError::InvalidMetadata(format!(
                    "metadata value too long for key: {k}"
                )));
            }
        }

        Ok(Self {
            id,
            tenant_id,
            mode,
            status: SessionStatus::Active,
            owner_subject_id,
            created_by,
            created_at,
            expires_at,
            max_members,
            snapshot_ttl_seconds,
            stale_after_seconds,
            metadata,
            closed_reason: None,
            version: 1,
        })
    }

    /// Evaluates effective status under lazy expiration check (INV-SES-2).
    ///
    /// If `status == ACTIVE` and `expires_at <= now`, it is treated as `EXPIRED`.
    pub fn effective_status(&self, now: OffsetDateTime) -> SessionStatus {
        if self.status == SessionStatus::Active && self.expires_at <= now {
            SessionStatus::Expired
        } else {
            self.status
        }
    }

    /// Checks if session is active at the given timestamp (INV-SES-1, INV-SES-2).
    pub fn is_active_at(&self, now: OffsetDateTime) -> bool {
        self.effective_status(now) == SessionStatus::Active
    }

    /// Closes the session (INV-SES-1, INV-SES-3).
    ///
    /// Idempotent: closing an already CLOSED or EXPIRED session returns Ok(false).
    /// Returns Ok(true) if transition from ACTIVE to CLOSED occurred.
    pub fn close(
        &mut self,
        reason: ClosedReason,
        now: OffsetDateTime,
    ) -> Result<bool, SessionError> {
        let eff = self.effective_status(now);
        match eff {
            SessionStatus::Closed => Ok(false),
            SessionStatus::Expired => {
                // INV-SES-3: closing an expired session returns its state without error
                self.status = SessionStatus::Expired;
                Ok(false)
            }
            SessionStatus::Active => {
                self.status = SessionStatus::Closed;
                self.closed_reason = Some(reason);
                self.version += 1;
                Ok(true)
            }
        }
    }

    /// Extends session expiration (INV-SES-1).
    pub fn extend_expiration(
        &mut self,
        new_expires_at: OffsetDateTime,
        now: OffsetDateTime,
        max_session_ttl_seconds: u32,
    ) -> Result<(), SessionError> {
        if !self.is_active_at(now) {
            return Err(SessionError::InactiveSession);
        }

        let min_extension = now + time::Duration::seconds(60);
        let max_extension = now + time::Duration::seconds(max_session_ttl_seconds as i64);

        if new_expires_at <= min_extension || new_expires_at > max_extension {
            return Err(SessionError::InvalidExpiration(
                "new_expires_at must be > now + 60s and <= now + max_session_ttl_seconds"
                    .to_string(),
            ));
        }

        self.expires_at = new_expires_at;
        self.version += 1;
        Ok(())
    }

    /// Updates max_members bound.
    pub fn update_max_members(
        &mut self,
        new_max: u32,
        active_members_count: u32,
        max_members_cap: u32,
        now: OffsetDateTime,
    ) -> Result<(), SessionError> {
        if !self.is_active_at(now) {
            return Err(SessionError::InactiveSession);
        }

        if self.mode == SessionMode::OneToOne {
            return Err(SessionError::InvalidMaxMembers(
                "max_members is immutable in ONE_TO_ONE mode".to_string(),
            ));
        }

        if new_max < active_members_count {
            return Err(SessionError::InvalidMaxMembers(
                "new max_members cannot be less than active members count".to_string(),
            ));
        }

        if new_max < 2 || new_max > max_members_cap {
            return Err(SessionError::InvalidMaxMembers(
                "new max_members outside allowed tenant cap bounds".to_string(),
            ));
        }

        self.max_members = new_max;
        self.version += 1;
        Ok(())
    }
}
