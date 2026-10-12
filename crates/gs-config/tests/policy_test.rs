//! Tests for TenantPolicy parsing, defaults, ranges, and cross-validations (docs/17 §4, FR-ADM-002).
#![forbid(unsafe_code)]

use gs_config::error::PolicyError;
use gs_config::policy::TenantPolicy;

#[test]
fn test_default_policy_is_valid_and_matches_spec() {
    let policy = TenantPolicy::default();
    assert_eq!(policy.default_session_ttl_seconds, 86400);
    assert_eq!(policy.max_session_ttl_seconds, 604800);
    assert_eq!(policy.default_max_members_one_to_many, 100);
    assert_eq!(policy.default_max_members_many_to_many, 50);
    assert_eq!(policy.max_members_cap, 1000);
    assert_eq!(policy.snapshot_ttl_default_seconds, 600);
    assert_eq!(policy.snapshot_ttl_max_seconds, 86400);
    assert_eq!(policy.stale_after_seconds_default, 60);
    assert_eq!(policy.min_publish_interval_ms, 1000);
    assert_eq!(policy.publish_burst, 5);
    assert_eq!(policy.max_samples_per_publish, 1);
    assert_eq!(policy.max_samples_per_publish_http, 50);
    assert!((policy.max_horizontal_accuracy_m - 50000.0).abs() < f64::EPSILON);
    assert!(policy.reject_null_island);
    assert_eq!(policy.max_active_sessions_per_subject, 50);
    assert_eq!(policy.max_devices_per_subject, 10);
    assert_eq!(policy.invitation_max_ttl_seconds, 604800);
    assert_eq!(policy.invitation_max_uses, 100);
    assert_eq!(policy.max_webhooks_per_tenant, 10);
    assert_eq!(policy.max_geofences_per_tenant, 10000);
    assert_eq!(policy.max_geofences_per_session, 100);
    assert_eq!(policy.geofence_confirm_samples, 2);
    assert!((policy.geofence_exit_buffer_m - 20.0).abs() < f64::EPSILON);
    assert!((policy.geofence_max_accuracy_m - 100.0).abs() < f64::EPSILON);
    assert!(!policy.history_allowed);

    assert!(policy.validate().is_ok());
}

#[test]
fn test_partial_json_merges_with_defaults() {
    let json_str = r#"{
        "default_session_ttl_seconds": 3600,
        "max_session_ttl_seconds": 7200,
        "reject_null_island": false
    }"#;
    let policy = TenantPolicy::from_json_str(json_str).expect("valid partial json");
    assert_eq!(policy.default_session_ttl_seconds, 3600);
    assert_eq!(policy.max_session_ttl_seconds, 7200);
    assert!(!policy.reject_null_island);
    // Unspecified fields should keep default
    assert_eq!(policy.default_max_members_one_to_many, 100);
    assert_eq!(policy.max_members_cap, 1000);
}

#[test]
fn test_unknown_policy_key_is_rejected() {
    let json_str = r#"{
        "default_session_ttl_seconds": 3600,
        "unknown_custom_key": 42
    }"#;
    let err = TenantPolicy::from_json_str(json_str).unwrap_err();
    match err {
        PolicyError::UnknownKey(key) => assert_eq!(key, "unknown_custom_key"),
        other => panic!("expected UnknownKey, got {other:?}"),
    }
}

#[test]
fn test_policy_out_of_range_checks() {
    // max_session_ttl_seconds < 3600
    let err = TenantPolicy::from_json_str(r#"{"max_session_ttl_seconds": 100}"#).unwrap_err();
    match err {
        PolicyError::OutOfRange { key, .. } => assert_eq!(key, "max_session_ttl_seconds"),
        other => panic!("expected OutOfRange, got {other:?}"),
    }

    // min_publish_interval_ms < 200
    let err = TenantPolicy::from_json_str(r#"{"min_publish_interval_ms": 50}"#).unwrap_err();
    match err {
        PolicyError::OutOfRange { key, .. } => assert_eq!(key, "min_publish_interval_ms"),
        other => panic!("expected OutOfRange, got {other:?}"),
    }

    // publish_burst > 20
    let err = TenantPolicy::from_json_str(r#"{"publish_burst": 25}"#).unwrap_err();
    match err {
        PolicyError::OutOfRange { key, .. } => assert_eq!(key, "publish_burst"),
        other => panic!("expected OutOfRange, got {other:?}"),
    }

    // geofence_exit_buffer_m > 500.0
    let err = TenantPolicy::from_json_str(r#"{"geofence_exit_buffer_m": 600.0}"#).unwrap_err();
    match err {
        PolicyError::OutOfRange { key, .. } => assert_eq!(key, "geofence_exit_buffer_m"),
        other => panic!("expected OutOfRange, got {other:?}"),
    }
}

#[test]
fn test_policy_cross_validation_rules() {
    // default_session_ttl_seconds > max_session_ttl_seconds
    let json = r#"{
        "default_session_ttl_seconds": 700000,
        "max_session_ttl_seconds": 604800
    }"#;
    let err = TenantPolicy::from_json_str(json).unwrap_err();
    match err {
        PolicyError::OutOfRange { key, .. } => assert_eq!(key, "default_session_ttl_seconds"),
        other => panic!("expected OutOfRange/CrossValidation, got {other:?}"),
    }

    // default_max_members_one_to_many > max_members_cap
    let json = r#"{
        "default_max_members_one_to_many": 500,
        "max_members_cap": 200
    }"#;
    let err = TenantPolicy::from_json_str(json).unwrap_err();
    match err {
        PolicyError::OutOfRange { key, .. } => assert_eq!(key, "default_max_members_one_to_many"),
        other => panic!("expected OutOfRange/CrossValidation, got {other:?}"),
    }

    // snapshot_ttl_default_seconds > snapshot_ttl_max_seconds
    let json = r#"{
        "snapshot_ttl_default_seconds": 1000,
        "snapshot_ttl_max_seconds": 500
    }"#;
    let err = TenantPolicy::from_json_str(json).unwrap_err();
    match err {
        PolicyError::OutOfRange { key, .. } => assert_eq!(key, "snapshot_ttl_default_seconds"),
        other => panic!("expected OutOfRange/CrossValidation, got {other:?}"),
    }
}
