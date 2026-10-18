//! Vector-driven tests for the plausibility gate and reset semantics
//! (`contracts/test-vectors/tracking_estimates.json`).
//!
//! Only the deterministic cases are asserted here: the flag **set** for the
//! `TRK-PLAUS-*` cases and the reset reason/speed for `TRK-RESET-001`. The
//! numeric `TRK-FILT-*` ranges depend on the exact Kalman parameterisation and
//! are left to the integration harness.

use gs_track::{
    Motion, PlausibilityFlags, Sample, TrackConfig, TrackEventKind, TrackResetReason,
    TrackStateMachine,
};

#[allow(clippy::too_many_arguments)]
fn sample(
    sequence: u64,
    at_s: u64,
    lat: f64,
    lon: f64,
    accuracy: f64,
    motion: Motion,
    source: &'static str,
    is_mock: bool,
) -> Sample {
    Sample {
        sequence,
        sample_at_ms: at_s * 1000,
        lat,
        lon,
        horizontal_accuracy_m: accuracy,
        speed_mps: None,
        speed_accuracy_mps: None,
        heading_deg: None,
        is_mock,
        source,
        motion,
    }
}

fn flags_of(machine: &mut TrackStateMachine, samples: &[Sample]) -> PlausibilityFlags {
    let mut last = PlausibilityFlags::empty();
    for s in samples {
        let up = machine.apply(s);
        assert!(up.accepted, "sample must be accepted (D-10)");
        last = up.estimate.expect("estimate present").flags;
    }
    last
}

fn names(flags: PlausibilityFlags) -> Vec<&'static str> {
    let mut v = flags.to_vec();
    v.sort_unstable();
    v
}

#[test]
fn trk_plaus_001_excessive_speed_is_flagged_and_accepted() {
    let mut m = TrackStateMachine::new(TrackConfig::defaults());
    let flags = flags_of(
        &mut m,
        &[
            sample(1, 0, 8.9824, -79.5199, 8.0, Motion::Walking, "GPS", false),
            sample(2, 1, 8.98359, -79.5199, 8.0, Motion::Walking, "GPS", false),
        ],
    );
    assert!(
        flags.excessive_speed,
        "132 m/s walking must flag EXCESSIVE_SPEED"
    );
}

#[test]
fn trk_plaus_002_low_accuracy_alone() {
    let mut m = TrackStateMachine::new(TrackConfig::defaults());
    let flags = flags_of(
        &mut m,
        &[
            sample(1, 0, 8.9824, -79.5199, 8.0, Motion::Unknown, "GPS", false),
            sample(
                2,
                2,
                8.98268,
                -79.5199,
                250.0,
                Motion::Unknown,
                "GPS",
                false,
            ),
        ],
    );
    assert_eq!(names(flags), vec!["LOW_ACCURACY"]);
}

#[test]
fn trk_plaus_003_mock_and_network() {
    let mut m = TrackStateMachine::new(TrackConfig::defaults());
    let flags = flags_of(
        &mut m,
        &[sample(
            1,
            0,
            8.9824,
            -79.5199,
            8.0,
            Motion::Unknown,
            "NETWORK",
            true,
        )],
    );
    assert_eq!(names(flags), vec!["MOCK_FLAGGED", "NETWORK_ONLY"]);
}

#[test]
fn trk_plaus_004_frozen_fix_after_five_identical() {
    let mut m = TrackStateMachine::new(TrackConfig::defaults());
    let samples: Vec<Sample> = (0..5)
        .map(|i| {
            sample(
                i + 1,
                i * 2,
                8.9824,
                -79.5199,
                8.0,
                Motion::Unknown,
                "GPS",
                false,
            )
        })
        .collect();
    let flags = flags_of(&mut m, &samples);
    assert!(
        flags.frozen_fix,
        "five identical fixes must flag FROZEN_FIX"
    );
}

#[test]
fn trk_reset_001_gap_resets_and_does_not_extrapolate() {
    // `TRK-RESET-001` uses a 30 s gap; `docs/17` sets `track_reset_gap_s = 60`
    // by default, so the vector only triggers a reset with a lower threshold.
    // We set it explicitly to exercise the reset path the vector describes.
    let mut config = TrackConfig::defaults();
    config.reset_gap_s = 20.0;
    let mut m = TrackStateMachine::new(config);
    let _ = m.apply(&sample(
        1,
        0,
        8.9824,
        -79.5199,
        8.0,
        Motion::Unknown,
        "GPS",
        false,
    ));
    let up = m.apply(&sample(
        2,
        30,
        8.99004,
        -79.5199,
        8.0,
        Motion::Unknown,
        "GPS",
        false,
    ));
    assert!(up.accepted);
    assert_eq!(up.reset_reason, Some(TrackResetReason::Gap));
    let est = up.estimate.unwrap();
    assert_eq!(est.speed_mps, None, "after a reset speed is unknown, not 0");
    // position equals the input (no extrapolation across the gap)
    assert!((est.position_lat - 8.99004).abs() < 1e-6);
    assert!((est.position_lon - (-79.5199)).abs() < 1e-6);
}

#[test]
fn first_fix_is_tagged_first_fix_and_keeps_the_sample() {
    let mut m = TrackStateMachine::new(TrackConfig::defaults());
    let up = m.apply(&sample(
        1,
        0,
        8.9824,
        -79.5199,
        8.0,
        Motion::Unknown,
        "GPS",
        false,
    ));
    assert_eq!(up.reset_reason, Some(TrackResetReason::FirstFix));
    let est = up.estimate.unwrap();
    assert!((est.position_lat - 8.9824).abs() < 1e-9);
    assert!((est.position_lon - (-79.5199)).abs() < 1e-9);
    assert_eq!(est.speed_mps, None);
}

#[test]
fn trk_odo_001_accumulates_a_straight_displacement() {
    // covers: FR-TRK-004, docs/22 §2.5
    let mut m = TrackStateMachine::new(TrackConfig::defaults());
    let _ = m.apply(&sample(
        1,
        0,
        8.9824,
        -79.5199,
        8.0,
        Motion::Automotive,
        "GPS",
        false,
    ));
    let up = m.apply(&sample(
        2,
        10,
        8.98292,
        -79.5199,
        8.0,
        Motion::Automotive,
        "GPS",
        false,
    ));
    let est = up.estimate.unwrap();
    // 0.00052 degrees of latitude ~ 57.8 m.
    assert!(
        (est.odometer_m - 58.0).abs() < 1.5,
        "odometer was {}",
        est.odometer_m
    );
}

#[test]
fn trk_stop_001_dwell_emits_stop_started() {
    // covers: FR-TRK-005, docs/22 §2.6
    let mut m = TrackStateMachine::new(TrackConfig::defaults());
    let _ = m.apply(&sample(
        1,
        0,
        8.98240,
        -79.51990,
        8.0,
        Motion::Stationary,
        "GPS",
        false,
    ));
    let _ = m.apply(&sample(
        2,
        60,
        8.98244,
        -79.51987,
        8.0,
        Motion::Stationary,
        "GPS",
        false,
    ));
    let up = m.apply(&sample(
        3,
        130,
        8.98243,
        -79.51991,
        8.0,
        Motion::Stationary,
        "GPS",
        false,
    ));
    assert!(
        up.events.contains(&TrackEventKind::StopStarted),
        "dwell >= track_stop_min_duration_s must emit STOP_STARTED, got {:?}",
        up.events
    );
}

#[test]
fn trk_stop_002_exit_after_confirm_emits_stop_ended() {
    // covers: FR-TRK-005, docs/22 §2.6
    let mut m = TrackStateMachine::new(TrackConfig::defaults());
    let _ = m.apply(&sample(
        1,
        0,
        8.98240,
        -79.51990,
        8.0,
        Motion::Stationary,
        "GPS",
        false,
    ));
    let _ = m.apply(&sample(
        2,
        130,
        8.98243,
        -79.51991,
        8.0,
        Motion::Stationary,
        "GPS",
        false,
    ));
    let _ = m.apply(&sample(
        3,
        180,
        8.98440,
        -79.51990,
        8.0,
        Motion::Automotive,
        "GPS",
        false,
    ));
    let up = m.apply(&sample(
        4,
        185,
        8.98490,
        -79.51990,
        8.0,
        Motion::Automotive,
        "GPS",
        false,
    ));
    assert!(
        up.events.contains(&TrackEventKind::StopEnded),
        "exit confirm must emit STOP_ENDED, got {:?}",
        up.events
    );
}
