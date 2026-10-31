//! Backend-agnostic suite for the ephemeral store (ADR-0010).
//!
//! The in-memory implementation runs it in `memory_ephemeral_test.rs` and the Redis/Valkey
//! implementation in `redis_ephemeral_test.rs`, so both backends behave identically.
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_ephemeral::{EphemeralStore, InMemoryEphemeralStore, SequenceCas};

async fn suite<S: EphemeralStore>(store: &S, prefix: &str) {
    snapshot_lifecycle(store, prefix).await;
    sequence_cas_semantics(store, prefix).await;
    presence_ownership(store, prefix).await;
    single_use_ticket(store, prefix).await;
    membership_version(store, prefix).await;
    counter_read(store, prefix).await;
    rate_limit_counter(store, prefix).await;
}

async fn snapshot_lifecycle<S: EphemeralStore>(store: &S, prefix: &str) {
    // covers: ADR-0010
    store
        .set_snapshot(
            &key(prefix, "suite:snap"),
            b"payload",
            Duration::from_secs(60),
        )
        .await
        .expect("set snapshot");
    assert_eq!(
        store
            .get_snapshot(&key(prefix, "suite:snap"))
            .await
            .expect("get snapshot"),
        Some(b"payload".to_vec())
    );
    assert!(
        store
            .delete_snapshot(&key(prefix, "suite:snap"))
            .await
            .expect("delete")
    );
    assert_eq!(
        store
            .get_snapshot(&key(prefix, "suite:snap"))
            .await
            .expect("get"),
        None
    );
    assert!(
        !store
            .delete_snapshot(&key(prefix, "suite:snap"))
            .await
            .expect("delete again")
    );
}

async fn sequence_cas_semantics<S: EphemeralStore>(store: &S, prefix: &str) {
    // covers: ADR-0008, docs/09 §4
    assert_eq!(
        store
            .compare_and_set_sequence(&key(prefix, "suite:seq"), 0, 7)
            .await
            .expect("first write"),
        SequenceCas::Accepted { previous: 0 }
    );
    assert_eq!(
        store
            .compare_and_set_sequence(&key(prefix, "suite:seq"), 7, 7)
            .await
            .expect("duplicate"),
        SequenceCas::Duplicate { current: 7 }
    );
    assert_eq!(
        store
            .compare_and_set_sequence(&key(prefix, "suite:seq"), 7, 5)
            .await
            .expect("obsolete"),
        SequenceCas::Stale { current: 7 }
    );
    assert_eq!(
        store
            .compare_and_set_sequence(&key(prefix, "suite:seq"), 7, 9)
            .await
            .expect("accepted"),
        SequenceCas::Accepted { previous: 7 }
    );
    assert_eq!(
        store
            .set_if_greater(&key(prefix, "suite:seq"), 4)
            .await
            .expect("max"),
        9
    );
    assert_eq!(
        store
            .set_if_greater(&key(prefix, "suite:seq"), 12)
            .await
            .expect("max"),
        12
    );
}

async fn presence_ownership<S: EphemeralStore>(store: &S, prefix: &str) {
    // covers: ADR-0010, docs/11 §3
    store
        .set_presence(
            &key(prefix, "suite:presence"),
            &[("connection_id", "con_1"), ("instance_id", "i-1")],
            Duration::from_secs(30),
        )
        .await
        .expect("set presence");
    assert_eq!(
        store
            .get_presence(&key(prefix, "suite:presence"))
            .await
            .expect("get"),
        Some(vec![
            ("connection_id".to_string(), "con_1".to_string()),
            ("instance_id".to_string(), "i-1".to_string()),
        ])
    );
    assert!(
        !store
            .release_presence(&key(prefix, "suite:presence"), "other")
            .await
            .expect("release")
    );
    assert!(
        store
            .release_presence(&key(prefix, "suite:presence"), "con_1")
            .await
            .expect("release")
    );
    assert_eq!(
        store
            .get_presence(&key(prefix, "suite:presence"))
            .await
            .expect("get"),
        None
    );
}

async fn single_use_ticket<S: EphemeralStore>(store: &S, prefix: &str) {
    // covers: docs/06 §5
    store
        .set_ticket(
            &key(prefix, "suite:ticket"),
            b"grant",
            Duration::from_secs(30),
        )
        .await
        .expect("set ticket");
    assert_eq!(
        store
            .take_ticket(&key(prefix, "suite:ticket"))
            .await
            .expect("take"),
        Some(b"grant".to_vec())
    );
    assert_eq!(
        store
            .take_ticket(&key(prefix, "suite:ticket"))
            .await
            .expect("take again"),
        None
    );
}

async fn membership_version<S: EphemeralStore>(store: &S, prefix: &str) {
    // covers: ADR-0009, docs/11 §3
    assert_eq!(
        store
            .increment_membership_version(&key(prefix, "suite:mver"))
            .await
            .expect("increment"),
        1
    );
    assert_eq!(
        store
            .increment_membership_version(&key(prefix, "suite:mver"))
            .await
            .expect("increment"),
        2
    );
    assert_eq!(
        store
            .get_membership_version(&key(prefix, "suite:mver"))
            .await
            .expect("get"),
        Some(2)
    );
}

async fn counter_read<S: EphemeralStore>(store: &S, prefix: &str) {
    // covers: docs/08 §6.2 (`sequence_floor`), docs/11 §3
    assert_eq!(
        store
            .get_counter(&key(prefix, "suite:devseq"))
            .await
            .expect("get empty"),
        None
    );
    assert_eq!(
        store
            .set_if_greater(&key(prefix, "suite:devseq"), 41)
            .await
            .expect("set"),
        41
    );
    assert_eq!(
        store
            .get_counter(&key(prefix, "suite:devseq"))
            .await
            .expect("get"),
        Some(41)
    );
    // Reading must not modify the value.
    assert_eq!(
        store
            .get_counter(&key(prefix, "suite:devseq"))
            .await
            .expect("get again"),
        Some(41)
    );
    assert_eq!(
        store
            .get_counter(&key(prefix, "suite:devseq"))
            .await
            .expect("get once more"),
        Some(41)
    );
}

async fn rate_limit_counter<S: EphemeralStore>(store: &S, prefix: &str) {
    // covers: docs/11 §3
    assert_eq!(
        store
            .increment_counter(&key(prefix, "suite:rl"), Duration::from_secs(60))
            .await
            .expect("increment"),
        1
    );
    assert_eq!(
        store
            .increment_counter(&key(prefix, "suite:rl"), Duration::from_secs(60))
            .await
            .expect("increment"),
        2
    );
}

/// Runs every case of the suite against `store` (ADR-0010).
#[allow(dead_code)]
pub async fn run_suite<S: EphemeralStore>(store: &S) {
    suite(store, "").await;
}

/// Runs the suite with every key under `prefix`, so runs against a shared server never see
/// each other's keys.
#[allow(dead_code)]
pub async fn run_suite_with_prefix<S: EphemeralStore>(store: &S, prefix: &str) {
    suite(store, prefix).await;
}

/// Prefixes a suite key.
fn key(prefix: &str, name: &str) -> String {
    format!("{prefix}{name}")
}

/// Convenience constructor for the in-memory implementation.
#[allow(dead_code)]
pub fn memory_store() -> InMemoryEphemeralStore {
    InMemoryEphemeralStore::default()
}
