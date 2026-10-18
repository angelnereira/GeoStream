//! Canonical ErrorCode catalog and domain error mapping (docs/09 §6, FR-API-001).
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::ids::{ExternalSubjectIdError, IdParseError, InstallationIdError};
use crate::invitation::{InvitationCodeError, InvitationError};
use crate::location::{BatchPreconditionError, LocationValidationError};
use crate::membership::MembershipError;
use crate::session::SessionError;

/// Canonical error codes shared between REST Problem Details and stream protocol (docs/09 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Unauthenticated,
    TokenExpired,
    PermissionDenied,
    InsufficientScope,
    AccountDisabled,
    NotFound,
    Conflict,
    ValidationFailed,
    InvalidLocation,
    RateLimited,
    PayloadTooLarge,
    SessionClosed,
    SessionExpired,
    SessionFull,
    MemberRevoked,
    NotSharing,
    NotPublisher,
    InvitationInvalid,
    IdempotencyKeyReused,
    PreconditionFailed,
    ProtocolVersionUnsupported,
    ProtocolError,
    TooManySubscriptions,
    TemporarilyUnavailable,
    Internal,
    /// The capability is off in the tenant policy (`403 FEATURE_DISABLED`).
    FeatureDisabled,
    /// The subject has not granted the required consent purpose (`403 CONSENT_REQUIRED`).
    ConsentRequired,
    /// The explicit provider is down: never silently degrade (`503 PROVIDER_UNAVAILABLE`).
    ProviderUnavailable,
}

impl ErrorCode {
    /// String name in REST Problem Details (without `ERROR_CODE_` prefix).
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::TokenExpired => "TOKEN_EXPIRED",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::InsufficientScope => "INSUFFICIENT_SCOPE",
            Self::AccountDisabled => "ACCOUNT_DISABLED",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::ValidationFailed => "VALIDATION_FAILED",
            Self::InvalidLocation => "INVALID_LOCATION",
            Self::RateLimited => "RATE_LIMITED",
            Self::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            Self::SessionClosed => "SESSION_CLOSED",
            Self::SessionExpired => "SESSION_EXPIRED",
            Self::SessionFull => "SESSION_FULL",
            Self::MemberRevoked => "MEMBER_REVOKED",
            Self::NotSharing => "NOT_SHARING",
            Self::NotPublisher => "NOT_PUBLISHER",
            Self::InvitationInvalid => "INVITATION_INVALID",
            Self::IdempotencyKeyReused => "IDEMPOTENCY_KEY_REUSED",
            Self::PreconditionFailed => "PRECONDITION_FAILED",
            Self::ProtocolVersionUnsupported => "PROTOCOL_VERSION_UNSUPPORTED",
            Self::ProtocolError => "PROTOCOL_ERROR",
            Self::TooManySubscriptions => "TOO_MANY_SUBSCRIPTIONS",
            Self::TemporarilyUnavailable => "TEMPORARILY_UNAVAILABLE",
            Self::Internal => "INTERNAL",
            Self::FeatureDisabled => "FEATURE_DISABLED",
            Self::ConsentRequired => "CONSENT_REQUIRED",
            Self::ProviderUnavailable => "PROVIDER_UNAVAILABLE",
        }
    }

    /// Protobuf enum integer value (common.proto ErrorCode).
    pub const fn proto_number(&self) -> i32 {
        match self {
            Self::Unauthenticated => 1,
            Self::TokenExpired => 2,
            Self::PermissionDenied => 3,
            Self::InsufficientScope => 4,
            Self::AccountDisabled => 5,
            Self::NotFound => 6,
            Self::Conflict => 7,
            Self::ValidationFailed => 8,
            Self::InvalidLocation => 9,
            Self::RateLimited => 10,
            Self::PayloadTooLarge => 11,
            Self::SessionClosed => 12,
            Self::SessionExpired => 13,
            Self::SessionFull => 14,
            Self::MemberRevoked => 15,
            Self::NotSharing => 16,
            Self::NotPublisher => 17,
            Self::InvitationInvalid => 18,
            Self::IdempotencyKeyReused => 19,
            Self::PreconditionFailed => 20,
            Self::ProtocolVersionUnsupported => 21,
            Self::ProtocolError => 22,
            Self::TooManySubscriptions => 23,
            Self::TemporarilyUnavailable => 24,
            Self::Internal => 25,
            Self::FeatureDisabled => 26,
            Self::ConsentRequired => 27,
            Self::ProviderUnavailable => 28,
        }
    }

    /// Converts from Protobuf enum integer value.
    pub const fn from_proto_number(n: i32) -> Option<Self> {
        match n {
            1 => Some(Self::Unauthenticated),
            2 => Some(Self::TokenExpired),
            3 => Some(Self::PermissionDenied),
            4 => Some(Self::InsufficientScope),
            5 => Some(Self::AccountDisabled),
            6 => Some(Self::NotFound),
            7 => Some(Self::Conflict),
            8 => Some(Self::ValidationFailed),
            9 => Some(Self::InvalidLocation),
            10 => Some(Self::RateLimited),
            11 => Some(Self::PayloadTooLarge),
            12 => Some(Self::SessionClosed),
            13 => Some(Self::SessionExpired),
            14 => Some(Self::SessionFull),
            15 => Some(Self::MemberRevoked),
            16 => Some(Self::NotSharing),
            17 => Some(Self::NotPublisher),
            18 => Some(Self::InvitationInvalid),
            19 => Some(Self::IdempotencyKeyReused),
            20 => Some(Self::PreconditionFailed),
            21 => Some(Self::ProtocolVersionUnsupported),
            22 => Some(Self::ProtocolError),
            23 => Some(Self::TooManySubscriptions),
            24 => Some(Self::TemporarilyUnavailable),
            25 => Some(Self::Internal),
            26 => Some(Self::FeatureDisabled),
            27 => Some(Self::ConsentRequired),
            28 => Some(Self::ProviderUnavailable),
            _ => None,
        }
    }

    /// Standard HTTP status code per docs/09 §6. Returns None for WebSocket-only errors.
    pub const fn http_status(&self) -> Option<u16> {
        match self {
            Self::Unauthenticated | Self::TokenExpired => Some(401),
            Self::PermissionDenied
            | Self::InsufficientScope
            | Self::AccountDisabled
            | Self::MemberRevoked => Some(403),
            Self::NotFound | Self::InvitationInvalid => Some(404),
            Self::Conflict
            | Self::SessionClosed
            | Self::SessionExpired
            | Self::SessionFull
            | Self::NotSharing
            | Self::NotPublisher => Some(409),
            Self::ValidationFailed | Self::InvalidLocation | Self::IdempotencyKeyReused => {
                Some(422)
            }
            Self::PreconditionFailed => Some(412),
            Self::PayloadTooLarge => Some(413),
            Self::RateLimited => Some(429),
            Self::ProtocolVersionUnsupported => Some(400),
            Self::ProtocolError | Self::TooManySubscriptions => None,
            Self::TemporarilyUnavailable | Self::ProviderUnavailable => Some(503),
            Self::Internal => Some(500),
            Self::FeatureDisabled | Self::ConsentRequired => Some(403),
        }
    }

    /// Whether the client may retry the operation per docs/09 §6.
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Unauthenticated
                | Self::TokenExpired
                | Self::RateLimited
                | Self::PreconditionFailed
                | Self::ProtocolError
                | Self::TemporarilyUnavailable
                | Self::Internal
        )
    }

    /// Client-safe English message (docs/09 §7, <= 200 characters, no sensitive values).
    pub const fn default_message(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated: missing, invalid, or revoked credentials",
            Self::TokenExpired => "token expired: refresh required",
            Self::PermissionDenied => "permission denied: caller lacks required capability",
            Self::InsufficientScope => "insufficient scope: credential lacks required scope",
            Self::AccountDisabled => "account disabled: tenant, application, or subject disabled",
            Self::NotFound => "resource not found",
            Self::Conflict => "conflict: incompatible resource state",
            Self::ValidationFailed => "validation failed: invalid request parameters",
            Self::InvalidLocation => "invalid location: location sample validation failed",
            Self::RateLimited => "rate limited: request quota exceeded",
            Self::PayloadTooLarge => "payload too large: request body or batch exceeds limit",
            Self::SessionClosed => "session closed: session is closed",
            Self::SessionExpired => "session expired: session has expired",
            Self::SessionFull => "session full: maximum active members limit reached",
            Self::MemberRevoked => "member revoked: subject was revoked from this session",
            Self::NotSharing => "not sharing: publishing attempted without active sharing",
            Self::NotPublisher => "not publisher: another device is the active publisher",
            Self::InvitationInvalid => {
                "invitation invalid: code is invalid, expired, or fully redeemed"
            }
            Self::IdempotencyKeyReused => "idempotency key reused with differing payload",
            Self::PreconditionFailed => {
                "precondition failed: target resource version does not match If-Match"
            }
            Self::ProtocolVersionUnsupported => "protocol version unsupported: please upgrade SDK",
            Self::ProtocolError => "protocol error: malformed protocol frame or state transition",
            Self::TooManySubscriptions => "too many subscriptions: connection limit exceeded",
            Self::TemporarilyUnavailable => {
                "temporarily unavailable: service dependency unavailable"
            }
            Self::Internal => "internal server error",
            Self::FeatureDisabled => "feature disabled: capability is off in the tenant policy",
            Self::ConsentRequired => "consent required: subject has not granted the purpose",
            Self::ProviderUnavailable => "provider unavailable: the explicit provider is down",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ErrorCode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().strip_prefix("ERROR_CODE_").unwrap_or(s.trim());
        match normalized {
            "UNAUTHENTICATED" => Ok(Self::Unauthenticated),
            "TOKEN_EXPIRED" => Ok(Self::TokenExpired),
            "PERMISSION_DENIED" => Ok(Self::PermissionDenied),
            "INSUFFICIENT_SCOPE" => Ok(Self::InsufficientScope),
            "ACCOUNT_DISABLED" => Ok(Self::AccountDisabled),
            "NOT_FOUND" => Ok(Self::NotFound),
            "CONFLICT" => Ok(Self::Conflict),
            "VALIDATION_FAILED" => Ok(Self::ValidationFailed),
            "INVALID_LOCATION" => Ok(Self::InvalidLocation),
            "RATE_LIMITED" => Ok(Self::RateLimited),
            "PAYLOAD_TOO_LARGE" => Ok(Self::PayloadTooLarge),
            "SESSION_CLOSED" => Ok(Self::SessionClosed),
            "SESSION_EXPIRED" => Ok(Self::SessionExpired),
            "SESSION_FULL" => Ok(Self::SessionFull),
            "MEMBER_REVOKED" => Ok(Self::MemberRevoked),
            "NOT_SHARING" => Ok(Self::NotSharing),
            "NOT_PUBLISHER" => Ok(Self::NotPublisher),
            "INVITATION_INVALID" => Ok(Self::InvitationInvalid),
            "IDEMPOTENCY_KEY_REUSED" => Ok(Self::IdempotencyKeyReused),
            "PRECONDITION_FAILED" => Ok(Self::PreconditionFailed),
            "PROTOCOL_VERSION_UNSUPPORTED" => Ok(Self::ProtocolVersionUnsupported),
            "PROTOCOL_ERROR" => Ok(Self::ProtocolError),
            "TOO_MANY_SUBSCRIPTIONS" => Ok(Self::TooManySubscriptions),
            "TEMPORARILY_UNAVAILABLE" => Ok(Self::TemporarilyUnavailable),
            "INTERNAL" => Ok(Self::Internal),
            _ => Err(()),
        }
    }
}

/// Trait for domain errors that map to canonical `ErrorCode` and HTTP status.
pub trait AsErrorCode {
    fn error_code(&self) -> ErrorCode;

    fn http_status(&self) -> Option<u16> {
        self.error_code().http_status()
    }

    fn is_retryable(&self) -> bool {
        self.error_code().is_retryable()
    }

    fn safe_message(&self) -> String {
        self.error_code().default_message().to_string()
    }
}

impl AsErrorCode for ErrorCode {
    fn error_code(&self) -> ErrorCode {
        *self
    }
}

impl AsErrorCode for LocationValidationError {
    fn error_code(&self) -> ErrorCode {
        ErrorCode::InvalidLocation
    }

    fn safe_message(&self) -> String {
        self.to_string()
    }
}

impl AsErrorCode for BatchPreconditionError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::PayloadTooLarge => ErrorCode::PayloadTooLarge,
            Self::ValidationFailed => ErrorCode::ValidationFailed,
        }
    }

    fn safe_message(&self) -> String {
        self.to_string()
    }
}

impl AsErrorCode for IdParseError {
    fn error_code(&self) -> ErrorCode {
        ErrorCode::ValidationFailed
    }

    fn safe_message(&self) -> String {
        self.to_string()
    }
}

impl AsErrorCode for InstallationIdError {
    fn error_code(&self) -> ErrorCode {
        ErrorCode::ValidationFailed
    }

    fn safe_message(&self) -> String {
        self.to_string()
    }
}

impl AsErrorCode for ExternalSubjectIdError {
    fn error_code(&self) -> ErrorCode {
        ErrorCode::ValidationFailed
    }

    fn safe_message(&self) -> String {
        self.to_string()
    }
}

impl AsErrorCode for SessionError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::InactiveSession => ErrorCode::Conflict,
            Self::SessionClosed => ErrorCode::SessionClosed,
            Self::SessionExpired => ErrorCode::SessionExpired,
            Self::SessionFull => ErrorCode::SessionFull,
            Self::MaxMembersLessThanActive => ErrorCode::Conflict,
            Self::InvalidMaxMembers(_) => ErrorCode::ValidationFailed,
            Self::InvalidExpiration(_) => ErrorCode::ValidationFailed,
            Self::InvalidMetadata(_) => ErrorCode::ValidationFailed,
        }
    }

    fn safe_message(&self) -> String {
        self.to_string()
    }
}

impl AsErrorCode for MembershipError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::OwnerCapabilitiesImmutable
            | Self::CannotEscalateCapabilities
            | Self::CannotModifyOwnCapabilities
            | Self::PermissionDenied
            | Self::NotActiveMember
            | Self::OnlySelfCanActivateSharing
            | Self::MissingPublishCapability => ErrorCode::PermissionDenied,

            Self::AlreadyActiveMember => ErrorCode::Conflict,
            Self::RevokedMemberCannotRedeemInvitation => ErrorCode::InvitationInvalid,
            Self::SessionFull => ErrorCode::SessionFull,
            Self::NotSharing => ErrorCode::NotSharing,
            Self::NotPublisher => ErrorCode::NotPublisher,
            Self::MemberRevoked => ErrorCode::MemberRevoked,
        }
    }

    fn safe_message(&self) -> String {
        self.to_string()
    }
}

impl AsErrorCode for InvitationCodeError {
    fn error_code(&self) -> ErrorCode {
        ErrorCode::InvitationInvalid
    }

    fn safe_message(&self) -> String {
        self.to_string()
    }
}

impl AsErrorCode for InvitationError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::AlreadyRevoked | Self::Expired | Self::MaxUsesReached | Self::CodeError(_) => {
                ErrorCode::InvitationInvalid
            }
            Self::InvalidGrants => ErrorCode::PermissionDenied,
        }
    }

    fn safe_message(&self) -> String {
        self.to_string()
    }
}

/// Unified domain error type wrapping all specific domain failure modes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("location error: {0}")]
    Location(#[from] LocationValidationError),

    #[error("batch error: {0}")]
    Batch(#[from] BatchPreconditionError),

    #[error("id parse error: {0}")]
    IdParse(#[from] IdParseError),

    #[error("installation id error: {0}")]
    InstallationId(#[from] InstallationIdError),

    #[error("external subject id error: {0}")]
    ExternalSubjectId(#[from] ExternalSubjectIdError),

    #[error("session error: {0}")]
    Session(#[from] SessionError),

    #[error("membership error: {0}")]
    Membership(#[from] MembershipError),

    #[error("invitation code error: {0}")]
    InvitationCode(#[from] InvitationCodeError),

    #[error("invitation error: {0}")]
    Invitation(#[from] InvitationError),

    #[error("general error with code {0}: {1}")]
    General(ErrorCode, String),
}

impl AsErrorCode for DomainError {
    fn error_code(&self) -> ErrorCode {
        match self {
            Self::Location(e) => e.error_code(),
            Self::Batch(e) => e.error_code(),
            Self::IdParse(e) => e.error_code(),
            Self::InstallationId(e) => e.error_code(),
            Self::ExternalSubjectId(e) => e.error_code(),
            Self::Session(e) => e.error_code(),
            Self::Membership(e) => e.error_code(),
            Self::InvitationCode(e) => e.error_code(),
            Self::Invitation(e) => e.error_code(),
            Self::General(code, _) => *code,
        }
    }

    fn safe_message(&self) -> String {
        match self {
            Self::Location(e) => e.safe_message(),
            Self::Batch(e) => e.safe_message(),
            Self::IdParse(e) => e.safe_message(),
            Self::InstallationId(e) => e.safe_message(),
            Self::ExternalSubjectId(e) => e.safe_message(),
            Self::Session(e) => e.safe_message(),
            Self::Membership(e) => e.safe_message(),
            Self::InvitationCode(e) => e.safe_message(),
            Self::Invitation(e) => e.safe_message(),
            Self::General(_, msg) => msg.clone(),
        }
    }
}
