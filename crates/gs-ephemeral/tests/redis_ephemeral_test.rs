//! Runs the backend-agnostic ephemeral suite against Redis/Valkey, plus the properties that only
//! a shared server has (T-F6-01, ADR-0010).
//!
//! Needs a Redis/Valkey >= 7.2 server: `GS_TEST_REDIS_URL`, by default the one of
//! `deploy/docker-compose.dev.yml`. Every test works under its own key prefix and deletes it.
#![forbid(unsafe_code)]

mod common;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gs_ephemeral::{
    CheckpointCas, DEVICE_SEQUENCE_TTL, EphemeralError, EphemeralStore, MEMBERSHIP_VERSION_TTL,
    RedisEphemeralStore, SEQUENCE_TTL, SequenceCas,
};

const COMMAND_TIMEOUT: Duration = Duration::from_millis(500);

fn test_redis_url() -> String {
    std::env::var("GS_TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

/// A key prefix no other test run shares.
fn unique_prefix(test: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    format!("gs-test:{}:{nanos}:{test}:", std::process::id())
}

async fn connect() -> RedisEphemeralStore {
    RedisEphemeralStore::connect(&test_redis_url(), COMMAND_TIMEOUT)
        .await
        .expect("connect to the test Redis/Valkey (GS_TEST_REDIS_URL)")
}

async fn raw_connection() -> redis::aio::MultiplexedConnection {
    redis::Client::open(test_redis_url())
        .expect("client")
        .get_multiplexed_async_connection()
        .await
        .expect("raw connection")
}

/// Deletes every key of a test prefix.
async fn cleanup(prefix: &str) {
    let mut connection = raw_connection().await;
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(format!("{prefix}*"))
        .query_async(&mut connection)
        .await
        .expect("list keys");
    if !keys.is_empty() {
        let _: u64 = redis::cmd("DEL")
            .arg(&keys)
            .query_async(&mut connection)
            .await
            .expect("delete keys");
    }
}

async fn pttl(key: &str) -> i64 {
    redis::cmd("PTTL")
        .arg(key)
        .query_async(&mut raw_connection().await)
        .await
        .expect("pttl")
}

#[tokio::test]
async fn test_redis_store_passes_the_shared_suite() {
    // covers: T-F6-01, ADR-0010
    let prefix = unique_prefix("suite");
    let store = connect().await;
    common::ephemeral_suite::run_suite_with_prefix(&store, &prefix).await;
    assert!(store.is_ready());
    cleanup(&prefix).await;
}

#[tokio::test]
async fn test_redis_store_is_shared_between_instances() {
    // covers: FR-STR-009, ADR-0010
    let prefix = unique_prefix("shared");
    let first = connect().await;
    let second = connect().await;
    let key = format!("{prefix}snap");
    first
        .set_snapshot(&key, b"1", Duration::from_secs(30))
        .await
        .expect("set snapshot");
    assert_eq!(
        second.get_snapshot(&key).await.expect("get"),
        Some(b"1".to_vec())
    );
    assert!(second.delete_snapshot(&key).await.expect("delete"));
    assert_eq!(first.get_snapshot(&key).await.expect("get"), None);
    cleanup(&prefix).await;
}

#[tokio::test]
async fn test_fr_prs_003_presence_expires_within_its_ttl_after_an_abrupt_loss() {
    // covers: FR-PRS-003
    let prefix = unique_prefix("presence");
    let key = format!("{prefix}presence:dev_1");
    {
        // The instance writes presence and dies without releasing it.
        let crashed = connect().await;
        crashed
            .set_presence(
                &key,
                &[("connection_id", "con_1"), ("instance_id", "i-1")],
                Duration::from_millis(300),
            )
            .await
            .expect("set presence");
    }
    let survivor = connect().await;
    assert!(survivor.get_presence(&key).await.expect("get").is_some());
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(survivor.get_presence(&key).await.expect("get"), None);
    cleanup(&prefix).await;
}

#[tokio::test]
async fn test_presence_of_a_new_connection_replaces_every_field() {
    // covers: docs/04 §6.3
    let prefix = unique_prefix("presence-replace");
    let store = connect().await;
    let key = format!("{prefix}presence:dev_1");
    store
        .set_presence(
            &key,
            &[
                ("connection_id", "con_1"),
                ("instance_id", "i-1"),
                ("stale", "x"),
            ],
            Duration::from_secs(30),
        )
        .await
        .expect("set presence");
    store
        .set_presence(&key, &[("connection_id", "con_2")], Duration::from_secs(30))
        .await
        .expect("replace presence");
    assert_eq!(
        store.get_presence(&key).await.expect("get"),
        Some(vec![("connection_id".to_string(), "con_2".to_string())])
    );
    // The closing connection of the old owner must not delete the new presence.
    assert!(
        !store
            .release_presence(&key, "con_1")
            .await
            .expect("release")
    );
    assert!(
        store
            .release_presence(&key, "con_2")
            .await
            .expect("release")
    );
    cleanup(&prefix).await;
}

#[tokio::test]
async fn test_sequence_cas_accepts_exactly_one_of_concurrent_writers() {
    // covers: ADR-0008, docs/09 §4
    let prefix = unique_prefix("seq-race");
    let key = format!("{prefix}seq:ses_1:dev_1");
    let mut writers = tokio::task::JoinSet::new();
    for _ in 0..32 {
        let key = key.clone();
        writers.spawn(async move {
            let store = connect().await;
            store
                .compare_and_set_sequence(&key, 0, 1)
                .await
                .expect("cas")
        });
    }
    let mut accepted = 0;
    let mut duplicates = 0;
    while let Some(outcome) = writers.join_next().await {
        match outcome.expect("writer") {
            SequenceCas::Accepted { previous: 0 } => accepted += 1,
            SequenceCas::Duplicate { current: 1 } => duplicates += 1,
            other => panic!("unexpected outcome {other:?}"),
        }
    }
    assert_eq!((accepted, duplicates), (1, 31));
    cleanup(&prefix).await;
}

#[tokio::test]
async fn test_sequences_above_two_to_the_53_keep_their_precision() {
    // covers: docs/09 §4 (u64 sequences; Lua numbers are doubles)
    let prefix = unique_prefix("seq-precision");
    let store = connect().await;
    let key = format!("{prefix}seq:ses_1:dev_1");
    let near_max = u64::MAX - 1;
    assert_eq!(
        store
            .compare_and_set_sequence(&key, 0, near_max)
            .await
            .expect("cas"),
        SequenceCas::Accepted { previous: 0 }
    );
    assert_eq!(
        store
            .compare_and_set_sequence(&key, near_max, u64::MAX)
            .await
            .expect("cas"),
        SequenceCas::Accepted { previous: near_max }
    );
    assert_eq!(
        store
            .compare_and_set_sequence(&key, u64::MAX, near_max)
            .await
            .expect("cas"),
        SequenceCas::Stale { current: u64::MAX }
    );

    let devseq = format!("{prefix}devseq:dev_1");
    let just_above = (1u64 << 53) + 1;
    assert_eq!(
        store
            .set_if_greater(&devseq, just_above)
            .await
            .expect("max"),
        just_above
    );
    assert_eq!(
        store
            .set_if_greater(&devseq, 1u64 << 53)
            .await
            .expect("max"),
        just_above
    );
    cleanup(&prefix).await;
}

#[tokio::test]
async fn test_checkpoint_cas_detects_a_concurrent_writer_on_another_instance() {
    // covers: docs/22 §2.1, docs/09 §4-bis
    let prefix = unique_prefix("trk");
    let first = connect().await;
    let second = connect().await;
    let key = format!("{prefix}trk:dev_1");
    let ttl = Duration::from_secs(60);

    assert_eq!(
        first
            .compare_and_set_checkpoint(&key, 0, b"one", ttl)
            .await
            .expect("cas"),
        CheckpointCas::Applied { version: 1 }
    );
    assert_eq!(
        second
            .compare_and_set_checkpoint(&key, 1, b"two", ttl)
            .await
            .expect("cas"),
        CheckpointCas::Applied { version: 2 }
    );
    // `first` still believes the version is 1: it must not overwrite.
    assert_eq!(
        first
            .compare_and_set_checkpoint(&key, 1, b"stale", ttl)
            .await
            .expect("cas"),
        CheckpointCas::Conflict { current_version: 2 }
    );
    assert_eq!(
        first.get_checkpoint(&key).await.expect("get"),
        Some((b"two".to_vec(), 2))
    );
    assert_eq!(
        first
            .set_checkpoint(&key, b"three", ttl)
            .await
            .expect("set"),
        3
    );
    assert_eq!(
        second.get_checkpoint(&key).await.expect("get"),
        Some((b"three".to_vec(), 3))
    );
    cleanup(&prefix).await;
}

#[tokio::test]
async fn test_every_key_gets_the_ttl_of_its_keyspace_row() {
    // covers: docs/11 §3 ("Todas las claves tienen TTL")
    let prefix = unique_prefix("ttl");
    let store = connect().await;
    let day_ms = 24 * 60 * 60 * 1_000;

    let seq = format!("{prefix}seq");
    store
        .compare_and_set_sequence(&seq, 0, 1)
        .await
        .expect("cas");
    let ttl = pttl(&seq).await;
    assert!(
        ttl > (SEQUENCE_TTL.as_millis() as i64) - day_ms && ttl <= SEQUENCE_TTL.as_millis() as i64
    );

    let devseq = format!("{prefix}devseq");
    store.set_if_greater(&devseq, 1).await.expect("max");
    let ttl = pttl(&devseq).await;
    assert!(
        ttl > (DEVICE_SEQUENCE_TTL.as_millis() as i64) - day_ms
            && ttl <= DEVICE_SEQUENCE_TTL.as_millis() as i64
    );

    let mver = format!("{prefix}mver");
    store
        .increment_membership_version(&mver)
        .await
        .expect("incr");
    let ttl = pttl(&mver).await;
    assert!(
        ttl > (MEMBERSHIP_VERSION_TTL.as_millis() as i64) - day_ms
            && ttl <= MEMBERSHIP_VERSION_TTL.as_millis() as i64
    );

    for (key, write) in [
        ("snap", 0u8),
        ("ticket", 1),
        ("presence", 2),
        ("rl", 3),
        ("trk", 4),
    ] {
        let key = format!("{prefix}{key}");
        let ttl = Duration::from_secs(30);
        match write {
            0 => store.set_snapshot(&key, b"x", ttl).await.expect("set"),
            1 => store.set_ticket(&key, b"x", ttl).await.expect("set"),
            2 => store
                .set_presence(&key, &[("connection_id", "c")], ttl)
                .await
                .expect("set"),
            3 => {
                store.increment_counter(&key, ttl).await.expect("incr");
            }
            _ => {
                store.set_checkpoint(&key, b"x", ttl).await.expect("set");
            }
        }
        let remaining = pttl(&key).await;
        assert!(
            remaining > 0 && remaining <= 30_000,
            "{key} has PTTL {remaining}"
        );
    }
    cleanup(&prefix).await;
}

#[tokio::test]
async fn test_rate_limit_window_starts_at_the_first_increment() {
    // covers: docs/11 §3 (`{p}:rl:*`)
    let prefix = unique_prefix("rl-window");
    let store = connect().await;
    let key = format!("{prefix}rl");
    let window = Duration::from_millis(400);
    assert_eq!(store.increment_counter(&key, window).await.expect("1"), 1);
    tokio::time::sleep(Duration::from_millis(250)).await;
    // A later increment must not extend the window.
    assert_eq!(store.increment_counter(&key, window).await.expect("2"), 2);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(store.increment_counter(&key, window).await.expect("3"), 1);
    cleanup(&prefix).await;
}

#[tokio::test]
async fn test_unreachable_server_reports_unavailable_within_the_timeout() {
    // covers: docs/04 §7.6, NFR-RES-002 (detection)
    let started = std::time::Instant::now();
    let result = RedisEphemeralStore::connect("redis://127.0.0.1:1", COMMAND_TIMEOUT).await;
    assert!(matches!(result, Err(EphemeralError::Unavailable(_))));
    assert!(started.elapsed() < COMMAND_TIMEOUT * 4);
}

#[tokio::test]
async fn test_errors_never_echo_the_connection_url() {
    // covers: docs/10 §8.2
    let secret_url = "redis://default:s3cr3t-password@127.0.0.1:1";
    let error = RedisEphemeralStore::connect(secret_url, COMMAND_TIMEOUT)
        .await
        .expect_err("unreachable");
    let rendered = format!("{error} {error:?}");
    assert!(!rendered.contains("s3cr3t-password"), "{rendered}");
}
