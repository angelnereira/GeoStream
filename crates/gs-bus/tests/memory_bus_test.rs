//! Runs the backend-agnostic bus suite against the in-memory implementation (T-F2-05).
#![forbid(unsafe_code)]

mod common;

use gs_bus::{Bus, InMemoryBus};

#[tokio::test]
async fn test_in_memory_bus_passes_the_shared_suite() {
    // covers: T-F2-05, ADR-0010
    common::bus_suite::run_suite(&InMemoryBus::new()).await;
}

#[tokio::test]
async fn test_bus_is_not_shared_between_instances() {
    // covers: ADR-0010
    let first = InMemoryBus::new();
    let second = InMemoryBus::new();
    let subscription = first.subscribe("instance:topic").await.expect("subscribe");
    assert_eq!(
        first
            .publish("instance:topic", b"event")
            .await
            .expect("publish"),
        1
    );
    assert_eq!(
        second
            .publish("instance:topic", b"event")
            .await
            .expect("publish"),
        0
    );
    assert!(first.unsubscribe(subscription).await.expect("unsubscribe"));
}

#[tokio::test]
async fn test_subscription_stops_receiving_after_unsubscribe() {
    // covers: docs/04 §6.1
    let bus = InMemoryBus::new();
    let subscription = bus.subscribe("suite:closed").await.expect("subscribe");
    assert!(bus.unsubscribe(subscription).await.expect("unsubscribe"));

    // Unsubscribing twice is an error and no message can arrive afterwards.
    assert!(bus.unsubscribe(subscription).await.is_err());
    assert!(bus.next_message(subscription).await.is_err());
}
