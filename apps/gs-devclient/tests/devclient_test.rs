//! Integration tests for gs-devclient (T-F4-12, M1 Demo).
#![forbid(unsafe_code)]

use gs_devclient::demo::{DemoConfig, run_demo};
use gs_devclient::trajectory::{TrajectoryGenerator, TrajectoryKind};
use gs_domain::Subprotocol;

#[test]
fn test_trajectory_generator_shapes() {
    // 1. Circle
    let mut circle_gen = TrajectoryGenerator::new(
        TrajectoryKind::Circle,
        -12.0464,
        -77.0428,
        15.0,
        500,
    )
    .with_radius(250.0);

    let mut prev_seq = 0;
    for _ in 0..10 {
        let sample = circle_gen.next_sample();
        assert!(sample.sequence > prev_seq);
        prev_seq = sample.sequence;

        let pos = sample.position.expect("position must be present");
        assert!(pos.latitude >= -90.0 && pos.latitude <= 90.0);
        assert!(pos.longitude >= -180.0 && pos.longitude <= 180.0);
        assert_eq!(pos.horizontal_accuracy_m, 5.0);
        assert!(pos.speed_mps.unwrap_or(0.0) > 0.0);
        assert!(pos.heading_deg.unwrap_or(0.0) >= 0.0 && pos.heading_deg.unwrap_or(0.0) < 360.0);
    }

    // 2. Linear
    let mut linear_gen = TrajectoryGenerator::new(
        TrajectoryKind::Linear,
        40.7128,
        -74.0060,
        20.0,
        1000,
    );
    let s1 = linear_gen.next_sample();
    let s2 = linear_gen.next_sample();
    let pos1 = s1.position.unwrap();
    let pos2 = s2.position.unwrap();
    assert!(pos2.latitude > pos1.latitude);
    assert!(pos2.longitude > pos1.longitude);

    // 3. Sine
    let mut sine_gen = TrajectoryGenerator::new(
        TrajectoryKind::Sine,
        51.5074,
        -0.1278,
        10.0,
        1000,
    );
    let mut sine_samples = Vec::new();
    for _ in 0..5 {
        sine_samples.push(sine_gen.next_sample());
    }
    assert_eq!(sine_samples.len(), 5);
}

#[tokio::test]
async fn test_devclient_demo_m1_proto_subprotocol() {
    let config = DemoConfig {
        duration_secs: 3,
        subprotocol: Subprotocol::Proto,
        quiet: true,
    };

    let stats = run_demo(config)
        .await
        .expect("M1 demo with gs.v1.proto must succeed");

    assert!(stats.alpha_sent > 0, "Courier Alpha must have sent samples");
    assert!(stats.beta_sent > 0, "Courier Beta must have sent samples");
    assert!(
        stats.dispatcher_received > 0,
        "Dispatcher must have received samples"
    );
    assert_eq!(
        stats.publishers_observed.len(),
        2,
        "Dispatcher must have observed both Courier Alpha and Courier Beta"
    );
}

#[tokio::test]
async fn test_devclient_demo_m1_json_subprotocol() {
    let config = DemoConfig {
        duration_secs: 3,
        subprotocol: Subprotocol::Json,
        quiet: true,
    };

    let stats = run_demo(config)
        .await
        .expect("M1 demo with gs.v1.json must succeed");

    assert!(stats.alpha_sent > 0, "Courier Alpha must have sent samples");
    assert!(stats.beta_sent > 0, "Courier Beta must have sent samples");
    assert!(
        stats.dispatcher_received > 0,
        "Dispatcher must have received samples"
    );
    assert_eq!(
        stats.publishers_observed.len(),
        2,
        "Dispatcher must have observed both Courier Alpha and Courier Beta"
    );
}
