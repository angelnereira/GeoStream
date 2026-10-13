//! Tracing, metrics and redacted types.
//!
//! Three responsibilities, all governed by `docs/10_SECURITY_PRIVACY.md` §8 and
//! `docs/16_OPERATIONS.md` §4: secret newtypes that cannot be logged, a metric and label
//! catalog that cannot leak identifiers, and JSON logs with the documented fields plus the
//! verifier that proves nothing forbidden was written.
#![forbid(unsafe_code)]

pub mod log_verifier;
pub mod logging;
pub mod metrics;
pub mod redacted;

pub use log_verifier::{
    CaptureReport, FORBIDDEN_PATTERNS, Finding, KNOWN_TEST_COORDINATES, assert_clean_capture,
    verify_capture,
};
pub use logging::{
    JsonLogFormatter, LogFields, LogFormat, LoggingError, init_from_env, logging_policy,
    write_json_line,
};
pub use metrics::{
    ALLOWED_LABEL_NAMES, AuthzCacheEvent, Labels, PrincipalKind, RateLimitScope, SampleResult,
    StatusClass, TopicKind, increment_counter, is_allowed_label, observe_duration,
    record_build_info, set_gauge,
};
pub use redacted::{AccessToken, ApiKeySecret, InvitationCode, Ticket, WebhookSecret, forget};
