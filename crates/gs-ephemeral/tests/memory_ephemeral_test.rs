//! Runs the backend-agnostic ephemeral suite against the in-memory implementation (T-F2-05).
#![forbid(unsafe_code)]

mod common;

use std::time::Duration;

use gs_ephemeral::{EphemeralStore, InMemoryEphemeralStore};

#[tokio::test]
async fn test_in_memory_store_passes_the_shared_suite() {
    // covers: T-F2-05, ADR-0010
    common::ephemeral_suite::run_suite(&common::ephemeral_suite::memory_store()).await;
}

#[tokio::test]
async fn test_in_memory_store_is_not_shared_between_instances() {
    // covers: ADR-0010
    let first = common::ephemeral_suite::memory_store();
    let second = InMemoryEphemeralStore::default();
    first
        .set_snapshot("instance:a", b"1", Duration::from_secs(30))
        .await
        .expect("set snapshot");
    assert_eq!(second.get_snapshot("instance:a").await.expect("get"), None);
}
