//! Tests for ErrorCode catalog parity and unambiguous domain error mappings (docs/09 §6, FR-API-001).
#![forbid(unsafe_code)]

use gs_domain::error::{AsErrorCode, DomainError, ErrorCode};
use gs_domain::ids::{ExternalSubjectIdError, IdParseError, InstallationIdError};
use gs_domain::invitation::{InvitationCodeError, InvitationError};
use gs_domain::location::{BatchPreconditionError, LocationValidationError};
use gs_domain::membership::MembershipError;
use gs_domain::session::SessionError;
use std::str::FromStr;

const ALL_25_ERROR_CODES: &[ErrorCode] = &[
    ErrorCode::Unauthenticated,
    ErrorCode::TokenExpired,
    ErrorCode::PermissionDenied,
    ErrorCode::InsufficientScope,
    ErrorCode::AccountDisabled,
    ErrorCode::NotFound,
    ErrorCode::Conflict,
    ErrorCode::ValidationFailed,
    ErrorCode::InvalidLocation,
    ErrorCode::RateLimited,
    ErrorCode::PayloadTooLarge,
    ErrorCode::SessionClosed,
    ErrorCode::SessionExpired,
    ErrorCode::SessionFull,
    ErrorCode::MemberRevoked,
    ErrorCode::NotSharing,
    ErrorCode::NotPublisher,
    ErrorCode::InvitationInvalid,
    ErrorCode::IdempotencyKeyReused,
    ErrorCode::PreconditionFailed,
    ErrorCode::ProtocolVersionUnsupported,
    ErrorCode::ProtocolError,
    ErrorCode::TooManySubscriptions,
    ErrorCode::TemporarilyUnavailable,
    ErrorCode::Internal,
];

#[test]
fn test_all_25_error_codes_roundtrip_names_and_proto_numbers() {
    assert_eq!(ALL_25_ERROR_CODES.len(), 25);

    for (idx, code) in ALL_25_ERROR_CODES.iter().enumerate() {
        let expected_proto = (idx + 1) as i32;
        assert_eq!(code.proto_number(), expected_proto);
        assert_eq!(ErrorCode::from_proto_number(expected_proto), Some(*code));

        let name = code.as_str();
        assert_eq!(ErrorCode::from_str(name), Ok(*code));
        let with_prefix = format!("ERROR_CODE_{name}");
        assert_eq!(ErrorCode::from_str(&with_prefix), Ok(*code));

        let msg = code.default_message();
        assert!(!msg.is_empty());
        assert!(
            msg.len() <= 200,
            "message for {code:?} must be <= 200 chars"
        );
    }
}

#[test]
fn test_http_status_codes_match_normative_catalog() {
    assert_eq!(ErrorCode::Unauthenticated.http_status(), Some(401));
    assert_eq!(ErrorCode::TokenExpired.http_status(), Some(401));
    assert_eq!(ErrorCode::PermissionDenied.http_status(), Some(403));
    assert_eq!(ErrorCode::InsufficientScope.http_status(), Some(403));
    assert_eq!(ErrorCode::AccountDisabled.http_status(), Some(403));
    assert_eq!(ErrorCode::NotFound.http_status(), Some(404));
    assert_eq!(ErrorCode::Conflict.http_status(), Some(409));
    assert_eq!(ErrorCode::ValidationFailed.http_status(), Some(422));
    assert_eq!(ErrorCode::InvalidLocation.http_status(), Some(422));
    assert_eq!(ErrorCode::RateLimited.http_status(), Some(429));
    assert_eq!(ErrorCode::PayloadTooLarge.http_status(), Some(413));
    assert_eq!(ErrorCode::SessionClosed.http_status(), Some(409));
    assert_eq!(ErrorCode::SessionExpired.http_status(), Some(409));
    assert_eq!(ErrorCode::SessionFull.http_status(), Some(409));
    assert_eq!(ErrorCode::MemberRevoked.http_status(), Some(403));
    assert_eq!(ErrorCode::NotSharing.http_status(), Some(409));
    assert_eq!(ErrorCode::NotPublisher.http_status(), Some(409));
    assert_eq!(ErrorCode::InvitationInvalid.http_status(), Some(404));
    assert_eq!(ErrorCode::IdempotencyKeyReused.http_status(), Some(422));
    assert_eq!(ErrorCode::PreconditionFailed.http_status(), Some(412));
    assert_eq!(
        ErrorCode::ProtocolVersionUnsupported.http_status(),
        Some(400)
    );
    assert_eq!(ErrorCode::ProtocolError.http_status(), None);
    assert_eq!(ErrorCode::TooManySubscriptions.http_status(), None);
    assert_eq!(ErrorCode::TemporarilyUnavailable.http_status(), Some(503));
    assert_eq!(ErrorCode::Internal.http_status(), Some(500));
}

#[test]
fn test_retryable_classification_matches_spec() {
    assert!(ErrorCode::Unauthenticated.is_retryable());
    assert!(ErrorCode::TokenExpired.is_retryable());
    assert!(ErrorCode::RateLimited.is_retryable());
    assert!(ErrorCode::PreconditionFailed.is_retryable());
    assert!(ErrorCode::ProtocolError.is_retryable());
    assert!(ErrorCode::TemporarilyUnavailable.is_retryable());
    assert!(ErrorCode::Internal.is_retryable());

    assert!(!ErrorCode::PermissionDenied.is_retryable());
    assert!(!ErrorCode::InsufficientScope.is_retryable());
    assert!(!ErrorCode::NotFound.is_retryable());
    assert!(!ErrorCode::ValidationFailed.is_retryable());
    assert!(!ErrorCode::SessionClosed.is_retryable());
}

#[test]
fn test_domain_error_mapping_to_error_code() {
    // Location validation errors
    let loc_err = LocationValidationError::new("position.latitude");
    assert_eq!(loc_err.error_code(), ErrorCode::InvalidLocation);
    assert_eq!(loc_err.http_status(), Some(422));

    // Batch precondition errors
    assert_eq!(
        BatchPreconditionError::PayloadTooLarge.error_code(),
        ErrorCode::PayloadTooLarge
    );
    assert_eq!(
        BatchPreconditionError::ValidationFailed.error_code(),
        ErrorCode::ValidationFailed
    );

    // ID parse errors
    let id_err = IdParseError::InvalidSeparator;
    assert_eq!(id_err.error_code(), ErrorCode::ValidationFailed);

    let install_err = InstallationIdError::InvalidFormat;
    assert_eq!(install_err.error_code(), ErrorCode::ValidationFailed);

    let ext_err = ExternalSubjectIdError::Empty;
    assert_eq!(ext_err.error_code(), ErrorCode::ValidationFailed);

    // Session errors
    assert_eq!(
        SessionError::InactiveSession.error_code(),
        ErrorCode::Conflict
    );
    assert_eq!(
        SessionError::SessionClosed.error_code(),
        ErrorCode::SessionClosed
    );
    assert_eq!(
        SessionError::SessionExpired.error_code(),
        ErrorCode::SessionExpired
    );
    assert_eq!(
        SessionError::SessionFull.error_code(),
        ErrorCode::SessionFull
    );
    assert_eq!(
        SessionError::MaxMembersLessThanActive.error_code(),
        ErrorCode::Conflict
    );

    // Membership errors
    assert_eq!(
        MembershipError::OwnerCapabilitiesImmutable.error_code(),
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        MembershipError::CannotEscalateCapabilities.error_code(),
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        MembershipError::CannotModifyOwnCapabilities.error_code(),
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        MembershipError::OnlySelfCanActivateSharing.error_code(),
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        MembershipError::AlreadyActiveMember.error_code(),
        ErrorCode::Conflict
    );
    assert_eq!(
        MembershipError::SessionFull.error_code(),
        ErrorCode::SessionFull
    );
    assert_eq!(
        MembershipError::NotSharing.error_code(),
        ErrorCode::NotSharing
    );
    assert_eq!(
        MembershipError::NotPublisher.error_code(),
        ErrorCode::NotPublisher
    );
    assert_eq!(
        MembershipError::MemberRevoked.error_code(),
        ErrorCode::MemberRevoked
    );
    assert_eq!(
        MembershipError::RevokedMemberCannotRedeemInvitation.error_code(),
        ErrorCode::InvitationInvalid
    );

    // Invitation errors
    assert_eq!(
        InvitationCodeError::InvalidInputLength.error_code(),
        ErrorCode::InvitationInvalid
    );
    assert_eq!(
        InvitationError::Expired.error_code(),
        ErrorCode::InvitationInvalid
    );
    assert_eq!(
        InvitationError::MaxUsesReached.error_code(),
        ErrorCode::InvitationInvalid
    );
    assert_eq!(
        InvitationError::AlreadyRevoked.error_code(),
        ErrorCode::InvitationInvalid
    );
    assert_eq!(
        InvitationError::InvalidGrants.error_code(),
        ErrorCode::PermissionDenied
    );
}

#[test]
fn test_unified_domain_error_conversions() {
    let domain_err: DomainError = SessionError::SessionClosed.into();
    assert_eq!(domain_err.error_code(), ErrorCode::SessionClosed);
    assert_eq!(domain_err.http_status(), Some(409));

    let domain_err: DomainError = BatchPreconditionError::PayloadTooLarge.into();
    assert_eq!(domain_err.error_code(), ErrorCode::PayloadTooLarge);
    assert_eq!(domain_err.http_status(), Some(413));

    let general = DomainError::General(ErrorCode::RateLimited, "too many requests".to_string());
    assert_eq!(general.error_code(), ErrorCode::RateLimited);
    assert_eq!(general.http_status(), Some(429));
    assert!(general.is_retryable());
}
