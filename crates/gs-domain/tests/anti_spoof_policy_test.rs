//! Reglas 17 y 18 de `09` §3.1 (DR-20261031-02) y umbral de precisión estricto (DR-20261031-03).
#![forbid(unsafe_code)]

use gs_domain::location::{
    LocationKind, LocationPolicy, LocationSource, RawLocationSample, RawPosition, RawQuality,
    validate_sample,
};
use time::OffsetDateTime;

fn sample(
    accuracy: f64,
    source: Option<LocationSource>,
    is_mock: Option<bool>,
) -> RawLocationSample {
    let now = OffsetDateTime::now_utc();
    RawLocationSample {
        sequence: Some(1),
        sequence_invalid: false,
        captured_at: Some(now),
        captured_at_invalid: false,
        position: Some(RawPosition {
            // Coordenadas ilustrativas (centro de Ciudad de Panamá), nunca de una persona.
            latitude: Some(8.98),
            longitude: Some(-79.52),
            horizontal_accuracy_m: Some(accuracy),
            ..RawPosition::default()
        }),
        quality: Some(RawQuality {
            source,
            kind: Some(LocationKind::Measured),
            is_mock,
            motion: None,
        }),
    }
}

fn field_of(s: &RawLocationSample, policy: &LocationPolicy) -> Option<&'static str> {
    validate_sample(s, policy, OffsetDateTime::now_utc())
        .err()
        .map(|e| e.field)
}

#[test]
fn test_defaults_accept_mock_and_network_samples() {
    let policy = LocationPolicy::default();
    assert_eq!(field_of(&sample(10.0, None, Some(true)), &policy), None);
    assert_eq!(
        field_of(&sample(10.0, Some(LocationSource::Network), None), &policy),
        None
    );
}

#[test]
fn test_rule_17_rejects_mock_when_policy_enabled() {
    let policy = LocationPolicy {
        reject_mock_samples: true,
        ..LocationPolicy::default()
    };
    assert_eq!(
        field_of(
            &sample(10.0, Some(LocationSource::Gnss), Some(true)),
            &policy
        ),
        Some("quality.is_mock")
    );
    assert_eq!(
        field_of(
            &sample(10.0, Some(LocationSource::Gnss), Some(false)),
            &policy
        ),
        None
    );
    assert_eq!(
        field_of(&sample(10.0, Some(LocationSource::Gnss), None), &policy),
        None
    );
}

#[test]
fn test_rule_18_rejects_network_only_when_policy_enabled() {
    let policy = LocationPolicy {
        reject_network_only: true,
        ..LocationPolicy::default()
    };
    assert_eq!(
        field_of(&sample(10.0, Some(LocationSource::Network), None), &policy),
        Some("quality.source")
    );
    for ok in [
        LocationSource::Gnss,
        LocationSource::Fused,
        LocationSource::Unspecified,
    ] {
        assert_eq!(field_of(&sample(10.0, Some(ok), None), &policy), None);
    }
}

#[test]
fn test_strict_accuracy_threshold_rejects_worse_samples() {
    let policy = LocationPolicy {
        max_horizontal_accuracy_m: 5.0,
        ..LocationPolicy::default()
    };
    assert_eq!(field_of(&sample(5.0, None, None), &policy), None);
    assert_eq!(
        field_of(&sample(5.1, None, None), &policy),
        Some("position.horizontal_accuracy_m")
    );
}

#[test]
fn test_accuracy_rule_runs_before_anti_spoof_rules() {
    // El orden de `09` §3.1 se mantiene: la regla 7 falla antes que la 17.
    let policy = LocationPolicy {
        max_horizontal_accuracy_m: 5.0,
        reject_mock_samples: true,
        ..LocationPolicy::default()
    };
    assert_eq!(
        field_of(&sample(50.0, None, Some(true)), &policy),
        Some("position.horizontal_accuracy_m")
    );
}
