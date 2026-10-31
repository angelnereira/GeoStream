//! Runs the backend-agnostic bus suite against Redis/Valkey Pub/Sub, plus the cross-instance
//! properties that the in-memory bus cannot have (T-F6-01, ADR-0010).
//!
//! Needs a Redis/Valkey >= 7.2 server: `GS_TEST_REDIS_URL`, by default the one of
//! `deploy/docker-compose.dev.yml`. Topics carry a per-run prefix.
#![forbid(unsafe_code)]

mod common;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gs_bus::{Bus, BusError, RedisBus};

const COMMAND_TIMEOUT: Duration = Duration::from_millis(500);

/// Upper bound to wait for a message that must arrive.
const DELIVERY_DEADLINE: Duration = Duration::from_secs(2);

fn test_redis_url() -> String {
    std::env::var("GS_TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

fn unique_topic(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    format!("gs-test:{}:{nanos}:bus:{name}", std::process::id())
}

async fn connect() -> RedisBus {
    RedisBus::connect(&test_redis_url(), COMMAND_TIMEOUT)
        .await
        .expect("connect to the test Redis/Valkey (GS_TEST_REDIS_URL)")
}

async fn receive(bus: &RedisBus, subscription: u64) -> Vec<u8> {
    tokio::time::timeout(DELIVERY_DEADLINE, bus.next_message(subscription))
        .await
        .expect("message within the deadline")
        .expect("receive")
        .expect("open subscription")
        .payload
}

#[tokio::test]
async fn test_redis_bus_passes_the_shared_suite() {
    // covers: T-F6-01, ADR-0010
    let bus = connect().await;
    common::bus_suite::run_suite(&bus).await;
    assert!(bus.is_connected());
}

#[tokio::test]
async fn test_fr_str_009_message_reaches_a_subscriber_on_another_instance() {
    // covers: FR-STR-009, docs/04 §6.1
    let publisher_instance = connect().await;
    let subscriber_instance = connect().await;
    let topic = unique_topic("loc");

    let subscription = subscriber_instance
        .subscribe(&topic)
        .await
        .expect("subscribe");
    // One instance holds a subscriber: Redis reports one receiving connection.
    assert_eq!(
        publisher_instance
            .publish(&topic, b"update")
            .await
            .expect("publish"),
        1
    );
    assert_eq!(receive(&subscriber_instance, subscription).await, b"update");
    assert_eq!(
        publisher_instance
            .subscription_count(&topic)
            .await
            .expect("count"),
        0
    );
    assert!(
        subscriber_instance
            .unsubscribe(subscription)
            .await
            .expect("unsubscribe")
    );
}

#[tokio::test]
async fn test_instance_unsubscribes_from_the_server_after_its_last_local_subscriber() {
    // covers: docs/04 §6.1 (refcount per topic)
    let publisher = connect().await;
    let subscriber = connect().await;
    let topic = unique_topic("refcount");

    let first = subscriber.subscribe(&topic).await.expect("subscribe");
    let second = subscriber.subscribe(&topic).await.expect("subscribe");
    // Two local subscriptions share one server subscription.
    assert_eq!(publisher.publish(&topic, b"a").await.expect("publish"), 1);
    assert_eq!(receive(&subscriber, first).await, b"a");
    assert_eq!(receive(&subscriber, second).await, b"a");

    assert!(subscriber.unsubscribe(first).await.expect("unsubscribe"));
    assert_eq!(publisher.publish(&topic, b"b").await.expect("publish"), 1);
    assert_eq!(receive(&subscriber, second).await, b"b");

    assert!(subscriber.unsubscribe(second).await.expect("unsubscribe"));
    assert_eq!(publisher.publish(&topic, b"c").await.expect("publish"), 0);
}

#[tokio::test]
async fn test_messages_arrive_in_publication_order() {
    // covers: docs/04 §6.1 (one route per session topic)
    let publisher = connect().await;
    let subscriber = connect().await;
    let topic = unique_topic("order");
    let subscription = subscriber.subscribe(&topic).await.expect("subscribe");
    for index in 0u32..100 {
        publisher
            .publish(&topic, &index.to_be_bytes())
            .await
            .expect("publish");
    }
    for index in 0u32..100 {
        assert_eq!(
            receive(&subscriber, subscription).await,
            index.to_be_bytes()
        );
    }
    assert!(
        subscriber
            .unsubscribe(subscription)
            .await
            .expect("unsubscribe")
    );
}

#[tokio::test]
async fn test_a_subscriber_that_falls_behind_reports_lagged() {
    // covers: docs/19 §3 (bounded channels), docs/04 §7.6 (no silent loss)
    let publisher = connect().await;
    let subscriber = connect().await;
    let topic = unique_topic("lagged");
    let subscription = subscriber.subscribe(&topic).await.expect("subscribe");

    // More than the per-subscription queue, without reading.
    for index in 0u32..1_500 {
        publisher
            .publish(&topic, &index.to_be_bytes())
            .await
            .expect("publish");
    }
    // Wait until the dispatcher has seen the tail.
    let deadline = tokio::time::Instant::now() + DELIVERY_DEADLINE;
    let mut lagged = None;
    while tokio::time::Instant::now() < deadline {
        match subscriber.next_message(subscription).await {
            Err(BusError::Lagged { dropped }) => {
                lagged = Some(dropped);
                break;
            }
            Ok(Some(_)) => {}
            other => panic!("unexpected receive {other:?}"),
        }
    }
    assert!(lagged.is_some_and(|dropped| dropped > 0));
    assert!(
        subscriber
            .unsubscribe(subscription)
            .await
            .expect("unsubscribe")
    );
}

#[tokio::test]
async fn test_unreachable_server_is_reported_and_does_not_echo_the_url() {
    // covers: docs/04 §7.6, docs/10 §8.2
    let error = RedisBus::connect(
        "redis://default:s3cr3t-password@127.0.0.1:1",
        COMMAND_TIMEOUT,
    )
    .await
    .expect_err("unreachable");
    assert!(matches!(error, BusError::ConnectionLost(_)));
    let rendered = format!("{error} {error:?}");
    assert!(!rendered.contains("s3cr3t-password"), "{rendered}");
}
