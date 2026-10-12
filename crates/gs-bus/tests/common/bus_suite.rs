//! Backend-agnostic suite for the bus (docs/04 §6.1, ADR-0010).
//!
//! The in-memory implementation runs it in `memory_bus_test.rs`; the Redis/Valkey Pub/Sub
//! implementation of T-F6-01 must run the same suite so both backends behave identically.
#![forbid(unsafe_code)]

use gs_bus::Bus;

async fn suite<B: Bus>(bus: &B) {
    publish_and_receive(bus).await;
    fan_out_to_every_subscriber(bus).await;
    reference_count_per_topic(bus).await;
    publish_without_subscribers_is_a_no_op(bus).await;
    unknown_subscriptions_are_rejected(bus).await;
    bus.is_connected();
}

async fn publish_and_receive<B: Bus>(bus: &B) {
    // covers: docs/04 §6.1, ADR-0010
    let subscription = bus.subscribe("suite:topic").await.expect("subscribe");
    let delivered = bus
        .publish("suite:topic", b"payload")
        .await
        .expect("publish");
    assert_eq!(delivered, 1);

    let message = bus
        .next_message(subscription)
        .await
        .expect("receive")
        .expect("message");
    assert_eq!(message.topic, "suite:topic");
    assert_eq!(message.payload, b"payload");

    assert!(bus.unsubscribe(subscription).await.expect("unsubscribe"));
}

async fn fan_out_to_every_subscriber<B: Bus>(bus: &B) {
    // covers: docs/04 §6.1
    let first = bus.subscribe("suite:fanout").await.expect("subscribe");
    let second = bus.subscribe("suite:fanout").await.expect("subscribe");

    assert_eq!(
        bus.publish("suite:fanout", b"event")
            .await
            .expect("publish"),
        2
    );
    for subscription in [first, second] {
        let message = bus
            .next_message(subscription)
            .await
            .expect("receive")
            .expect("message");
        assert_eq!(message.payload, b"event");
        assert!(bus.unsubscribe(subscription).await.expect("unsubscribe"));
    }
}

async fn reference_count_per_topic<B: Bus>(bus: &B) {
    // covers: docs/04 §6.1
    let first = bus.subscribe("suite:refcount").await.expect("subscribe");
    assert_eq!(
        bus.subscription_count("suite:refcount")
            .await
            .expect("count"),
        1
    );

    let second = bus.subscribe("suite:refcount").await.expect("subscribe");
    assert_eq!(
        bus.subscription_count("suite:refcount")
            .await
            .expect("count"),
        2
    );

    assert!(bus.unsubscribe(first).await.expect("unsubscribe"));
    assert_eq!(
        bus.subscription_count("suite:refcount")
            .await
            .expect("count"),
        1
    );
    assert!(bus.unsubscribe(second).await.expect("unsubscribe"));
    assert_eq!(
        bus.subscription_count("suite:refcount")
            .await
            .expect("count"),
        0
    );
}

async fn publish_without_subscribers_is_a_no_op<B: Bus>(bus: &B) {
    // covers: ADR-0010
    assert_eq!(
        bus.publish("suite:empty", b"nobody")
            .await
            .expect("publish"),
        0
    );
}

async fn unknown_subscriptions_are_rejected<B: Bus>(bus: &B) {
    // covers: docs/04 §6.1
    assert!(bus.unsubscribe(9_999).await.is_err());
    assert!(bus.next_message(9_999).await.is_err());
}

/// Runs every case of the suite against `bus` (ADR-0010).
pub async fn run_suite<B: Bus>(bus: &B) {
    suite(bus).await;
}
