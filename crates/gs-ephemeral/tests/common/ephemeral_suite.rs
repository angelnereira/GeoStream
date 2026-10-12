//! Backend-agnostic suite for the ephemeral store (ADR-0010).
//!
//! The in-memory implementation runs it in `memory_ephemeral_test.rs`; the Redis/Valkey
//! implementation of T-F6-01 must run the same suite so both backends behave identically.
#![forbid(unsafe_code)]

use std::time::Duration;

use gs_ephemeral::{EphemeralStore, InMemoryEphemeralStore, SequenceCas};

async fn suite<S: EphemeralStore>(store: &S) {
    snapshot_lifecycle(store).await;
    sequence_cas_semantics(store).await;
    presence_ownership(store).await;
    single_use_ticket(store).await;
    membership_version(store).await;
    rate_limit_counter(store).await;
}

async fn snapshot_lifecycle<S: EphemeralStore>(store: &S) {
    // covers: ADR-0010
    store
        .set_snapshot("suite:snap", b"payload", Duration::from_secs(60))
        .await
        .expect("set snapshot");
    assert_eq!(
        store
            .get_snapshot("suite:snap")
            .await
            .expect("get snapshot"),
        Some(b"payload".to_vec())
    );
    assert!(store.delete_snapshot("suite:snap").await.expect("delete"));
    assert_eq!(store.get_snapshot("suite:snap").await.expect("get"), None);
    assert!(
        !store
            .delete_snapshot("suite:snap")
            .await
            .expect("delete again")
    );
}

async fn sequence_cas_semantics<S: EphemeralStore>(store: &S) {
    // covers: ADR-0008, docs/09 §4
    assert_eq!(
        store
            .compare_and_set_sequence("suite:seq", 0, 7)
            .await
            .expect("first write"),
        SequenceCas::Accepted { previous: 0 }
    );
    assert_eq!(
        store
            .compare_and_set_sequence("suite:seq", 7, 7)
            .await
            .expect("duplicate"),
        SequenceCas::Duplicate { current: 7 }
    );
    assert_eq!(
        store
            .compare_and_set_sequence("suite:seq", 7, 5)
            .await
            .expect("obsolete"),
        SequenceCas::Stale { current: 7 }
    );
    assert_eq!(
        store
            .compare_and_set_sequence("suite:seq", 7, 9)
            .await
            .expect("accepted"),
        SequenceCas::Accepted { previous: 7 }
    );
    assert_eq!(store.set_if_greater("suite:seq", 4).await.expect("max"), 9);
    assert_eq!(
        store.set_if_greater("suite:seq", 12).await.expect("max"),
        12
    );
}

async fn presence_ownership<S: EphemeralStore>(store: &S) {
    // covers: ADR-0010, docs/11 §3
    store
        .set_presence(
            "suite:presence",
            &[("connection_id", "con_1"), ("instance_id", "i-1")],
            Duration::from_secs(30),
        )
        .await
        .expect("set presence");
    assert_eq!(
        store.get_presence("suite:presence").await.expect("get"),
        Some(vec![
            ("connection_id".to_string(), "con_1".to_string()),
            ("instance_id".to_string(), "i-1".to_string()),
        ])
    );
    assert!(
        !store
            .release_presence("suite:presence", "other")
            .await
            .expect("release")
    );
    assert!(
        store
            .release_presence("suite:presence", "con_1")
            .await
            .expect("release")
    );
    assert_eq!(
        store.get_presence("suite:presence").await.expect("get"),
        None
    );
}

async fn single_use_ticket<S: EphemeralStore>(store: &S) {
    // covers: docs/06 §5
    store
        .set_ticket("suite:ticket", b"grant", Duration::from_secs(30))
        .await
        .expect("set ticket");
    assert_eq!(
        store.take_ticket("suite:ticket").await.expect("take"),
        Some(b"grant".to_vec())
    );
    assert_eq!(
        store.take_ticket("suite:ticket").await.expect("take again"),
        None
    );
}

async fn membership_version<S: EphemeralStore>(store: &S) {
    // covers: ADR-0009, docs/11 §3
    assert_eq!(
        store
            .increment_membership_version("suite:mver")
            .await
            .expect("increment"),
        1
    );
    assert_eq!(
        store
            .increment_membership_version("suite:mver")
            .await
            .expect("increment"),
        2
    );
    assert_eq!(
        store
            .get_membership_version("suite:mver")
            .await
            .expect("get"),
        Some(2)
    );
}

async fn rate_limit_counter<S: EphemeralStore>(store: &S) {
    // covers: docs/11 §3
    assert_eq!(
        store
            .increment_counter("suite:rl", Duration::from_secs(60))
            .await
            .expect("increment"),
        1
    );
    assert_eq!(
        store
            .increment_counter("suite:rl", Duration::from_secs(60))
            .await
            .expect("increment"),
        2
    );
}

/// Runs every case of the suite against `store` (ADR-0010).
pub async fn run_suite<S: EphemeralStore>(store: &S) {
    suite(store).await;
}

/// Convenience constructor for the in-memory implementation.
pub fn memory_store() -> InMemoryEphemeralStore {
    InMemoryEphemeralStore::default()
}
