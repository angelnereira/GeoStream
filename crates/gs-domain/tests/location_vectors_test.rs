//! Tests for location sample validation and acceptance against contracts/test-vectors/location_samples.json.
//!
//! Validates FR-LOC-001, FR-LOC-002, FR-LOC-004, FR-LOC-006 (T-F1-02).
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use time::OffsetDateTime;

use gs_domain::location::{
    LocationPolicy, LocationValidationError, Position, RawLocationSample, evaluate_sample,
};

fn find_repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("Failed to locate repo root from CARGO_MANIFEST_DIR")
        .to_path_buf()
}

fn apply_patch(base: &mut serde_json::Value, patch: &serde_json::Map<String, serde_json::Value>) {
    for (key, val) in patch {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() == 1 {
            if val.is_null() {
                if let Some(obj) = base.as_object_mut() {
                    obj.remove(parts[0]);
                }
            } else if let Some(obj) = base.as_object_mut() {
                obj.insert(parts[0].to_string(), val.clone());
            }
        } else if parts.len() == 2
            && let Some(parent) = base.get_mut(parts[0]).and_then(|v| v.as_object_mut())
        {
            if val.is_null() {
                parent.remove(parts[1]);
            } else {
                parent.insert(parts[1].to_string(), val.clone());
            }
        }
    }
}

#[test]
fn test_all_location_sample_cases_from_vectors() {
    let repo_root = find_repo_root();
    let vector_path = repo_root.join("contracts/test-vectors/location_samples.json");
    let content = fs::read_to_string(&vector_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", vector_path.display()));
    let root: serde_json::Value =
        serde_json::from_str(&content).expect("Failed to parse location_samples.json");

    let server_now_str = root
        .get("server_now")
        .and_then(|v| v.as_str())
        .expect("server_now missing in vectors");
    let server_now = OffsetDateTime::parse(
        server_now_str,
        &time::format_description::well_known::Rfc3339,
    )
    .expect("server_now invalid RFC 3339");

    let default_last_sequence = root
        .get("default_last_sequence")
        .and_then(|v| v.as_u64())
        .unwrap_or(100);

    let default_policy_val = root
        .get("default_policy")
        .expect("default_policy missing in vectors");
    let default_policy = LocationPolicy {
        max_horizontal_accuracy_m: default_policy_val
            .get("max_horizontal_accuracy_m")
            .and_then(|v| v.as_f64())
            .unwrap_or(50_000.0),
        reject_null_island: default_policy_val
            .get("reject_null_island")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        max_future_skew_ms: default_policy_val
            .get("max_future_skew_ms")
            .and_then(|v| v.as_i64())
            .unwrap_or(5_000),
        max_event_age_ms: default_policy_val
            .get("max_event_age_ms")
            .and_then(|v| v.as_i64())
            .unwrap_or(120_000),
        max_samples_per_publish: default_policy_val
            .get("max_samples_per_publish")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as u32,
    };

    let base_sample = root
        .get("base_sample")
        .expect("base_sample missing in vectors")
        .clone();

    let cases = root
        .get("cases")
        .and_then(|v| v.as_array())
        .expect("cases array missing in vectors");

    assert_eq!(
        cases.len(),
        49,
        "Expected 49 cases in location_samples.json"
    );

    for case in cases {
        let id = case.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let desc = case
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let patch = case
            .get("patch")
            .and_then(|v| v.as_object())
            .expect("patch object missing in case");

        let mut sample_json = base_sample.clone();
        apply_patch(&mut sample_json, patch);

        let mut policy = default_policy.clone();
        if let Some(overrides) = case.get("policy").and_then(|p| p.as_object()) {
            if let Some(v) = overrides
                .get("max_horizontal_accuracy_m")
                .and_then(|v| v.as_f64())
            {
                policy.max_horizontal_accuracy_m = v;
            }
            if let Some(v) = overrides
                .get("reject_null_island")
                .and_then(|v| v.as_bool())
            {
                policy.reject_null_island = v;
            }
            if let Some(v) = overrides.get("max_future_skew_ms").and_then(|v| v.as_i64()) {
                policy.max_future_skew_ms = v;
            }
            if let Some(v) = overrides.get("max_event_age_ms").and_then(|v| v.as_i64()) {
                policy.max_event_age_ms = v;
            }
            if let Some(v) = overrides
                .get("max_samples_per_publish")
                .and_then(|v| v.as_u64())
            {
                policy.max_samples_per_publish = v as u32;
            }
        }

        let last_seq: Option<u64> = if let Some(last_val) = case.get("last_sequence") {
            last_val.as_u64()
        } else {
            Some(default_last_sequence)
        };

        let raw = RawLocationSample::from_json(&sample_json);
        let result = evaluate_sample(&raw, &policy, server_now, last_seq);

        let expect = case.get("expect").expect("expect missing in case");
        let expect_status = expect
            .get("status")
            .and_then(|v| v.as_str())
            .expect("expect.status missing");

        assert_eq!(
            result.status.to_string(),
            expect_status,
            "Case {id} ({desc}): expected status {expect_status}, got {}",
            result.status
        );

        if expect_status == "REJECTED" {
            let expect_error_code = expect
                .get("error_code")
                .and_then(|v| v.as_str())
                .expect("expect.error_code missing in REJECTED case");
            let expect_field = expect
                .get("field")
                .and_then(|v| v.as_str())
                .expect("expect.field missing in REJECTED case");

            assert_eq!(
                result.error_code.as_deref(),
                Some(expect_error_code),
                "Case {id} ({desc}): expected error_code {expect_error_code}, got {:?}",
                result.error_code
            );
            assert_eq!(
                result.field.as_deref(),
                Some(expect_field),
                "Case {id} ({desc}): expected field {expect_field}, got {:?}",
                result.field
            );
        }
    }
}

#[test]
fn test_coordinates_redaction_in_debug_and_display() {
    let policy = LocationPolicy::default();
    let pos = Position::new(
        8.9824,
        -79.5199,
        8.5,
        Some(15.0),
        Some(2.0),
        Some(3.5),
        Some(0.5),
        Some(120.0),
        Some(5.0),
        &policy,
    )
    .expect("Valid position");

    let debug_str = format!("{pos:?}");
    let display_str = format!("{pos}");

    assert_eq!(debug_str, "Position { <redacted> }");
    assert_eq!(display_str, "Position { <redacted> }");

    assert!(!debug_str.contains("8.9824"));
    assert!(!debug_str.contains("-79.5199"));
    assert!(!display_str.contains("8.9824"));
    assert!(!display_str.contains("-79.5199"));
}

#[test]
fn test_position_validation_standalone() {
    let policy = LocationPolicy::default();

    // Invalid latitude (> 90)
    let res = Position::new(91.0, 0.0, 10.0, None, None, None, None, None, None, &policy);
    assert_eq!(
        res.unwrap_err(),
        LocationValidationError::new("position.latitude")
    );

    // Null island with reject_null_island = true
    let res = Position::new(0.0, 0.0, 10.0, None, None, None, None, None, None, &policy);
    assert_eq!(res.unwrap_err(), LocationValidationError::new("position"));

    // Null island with reject_null_island = false
    let relaxed_policy = LocationPolicy {
        reject_null_island: false,
        ..policy
    };
    let res = Position::new(
        0.0,
        0.0,
        10.0,
        None,
        None,
        None,
        None,
        None,
        None,
        &relaxed_policy,
    );
    assert!(res.is_ok());
}
