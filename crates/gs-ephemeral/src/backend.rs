//! Runtime selection of the ephemeral backend (`GS_STATE_BACKEND`, docs/17 §2.3, ADR-0010).
//!
//! The trait uses `async fn`, so it cannot be a trait object. An enum keeps the futures
//! concrete (and therefore provably `Send` for the HTTP and stream handlers) while `serve`
//! chooses the backend from the configuration.

use std::sync::Arc;
use std::time::Duration;

use crate::{
    CheckpointCas, EphemeralError, EphemeralStore, InMemoryEphemeralStore, RedisEphemeralStore,
    SequenceCas,
};

/// The ephemeral store a process uses: in memory (one instance) or Redis/Valkey (shared).
#[derive(Debug, Clone)]
pub enum EphemeralBackend {
    /// `GS_STATE_BACKEND=memory`: a single instance only.
    Memory(Arc<InMemoryEphemeralStore>),
    /// `GS_STATE_BACKEND=redis`: shared by every instance.
    Redis(Arc<RedisEphemeralStore>),
}

impl EphemeralBackend {
    /// A new, empty in-memory backend.
    pub fn memory() -> Arc<Self> {
        Arc::new(Self::Memory(InMemoryEphemeralStore::new()))
    }

    /// Wraps an existing in-memory store, sharing its contents.
    pub fn from_memory(store: Arc<InMemoryEphemeralStore>) -> Arc<Self> {
        Arc::new(Self::Memory(store))
    }

    /// Wraps a connected Redis/Valkey store.
    pub fn from_redis(store: RedisEphemeralStore) -> Arc<Self> {
        Arc::new(Self::Redis(Arc::new(store)))
    }
}

/// Forwards one trait call to the selected backend.
macro_rules! dispatch {
    ($self:ident, $store:ident => $call:expr) => {
        match $self {
            EphemeralBackend::Memory($store) => $call,
            EphemeralBackend::Redis($store) => $call,
        }
    };
}

impl EphemeralStore for EphemeralBackend {
    async fn set_snapshot(
        &self,
        key: &str,
        value: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        dispatch!(self, store => store.set_snapshot(key, value, ttl).await)
    }

    async fn get_snapshot(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError> {
        dispatch!(self, store => store.get_snapshot(key).await)
    }

    async fn delete_snapshot(&self, key: &str) -> Result<bool, EphemeralError> {
        dispatch!(self, store => store.delete_snapshot(key).await)
    }

    async fn compare_and_set_sequence(
        &self,
        key: &str,
        current: u64,
        next: u64,
    ) -> Result<SequenceCas, EphemeralError> {
        dispatch!(self, store => store.compare_and_set_sequence(key, current, next).await)
    }

    async fn set_if_greater(&self, key: &str, value: u64) -> Result<u64, EphemeralError> {
        dispatch!(self, store => store.set_if_greater(key, value).await)
    }

    async fn set_presence(
        &self,
        key: &str,
        fields: &[(&str, &str)],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        dispatch!(self, store => store.set_presence(key, fields, ttl).await)
    }

    async fn get_presence(
        &self,
        key: &str,
    ) -> Result<Option<Vec<(String, String)>>, EphemeralError> {
        dispatch!(self, store => store.get_presence(key).await)
    }

    async fn release_presence(
        &self,
        key: &str,
        connection_id: &str,
    ) -> Result<bool, EphemeralError> {
        dispatch!(self, store => store.release_presence(key, connection_id).await)
    }

    async fn get_membership_version(&self, key: &str) -> Result<Option<u64>, EphemeralError> {
        dispatch!(self, store => store.get_membership_version(key).await)
    }

    async fn get_counter(&self, key: &str) -> Result<Option<u64>, EphemeralError> {
        dispatch!(self, store => store.get_counter(key).await)
    }

    async fn increment_membership_version(&self, key: &str) -> Result<u64, EphemeralError> {
        dispatch!(self, store => store.increment_membership_version(key).await)
    }

    async fn set_ticket(
        &self,
        key: &str,
        grant: &[u8],
        ttl: Duration,
    ) -> Result<(), EphemeralError> {
        dispatch!(self, store => store.set_ticket(key, grant, ttl).await)
    }

    async fn take_ticket(&self, key: &str) -> Result<Option<Vec<u8>>, EphemeralError> {
        dispatch!(self, store => store.take_ticket(key).await)
    }

    async fn increment_counter(&self, key: &str, window: Duration) -> Result<u64, EphemeralError> {
        dispatch!(self, store => store.increment_counter(key, window).await)
    }

    async fn set_checkpoint(
        &self,
        key: &str,
        value: &[u8],
        ttl: Duration,
    ) -> Result<u64, EphemeralError> {
        dispatch!(self, store => store.set_checkpoint(key, value, ttl).await)
    }

    async fn get_checkpoint(&self, key: &str) -> Result<Option<(Vec<u8>, u64)>, EphemeralError> {
        dispatch!(self, store => store.get_checkpoint(key).await)
    }

    async fn compare_and_set_checkpoint(
        &self,
        key: &str,
        expected_version: u64,
        value: &[u8],
        ttl: Duration,
    ) -> Result<CheckpointCas, EphemeralError> {
        dispatch!(self, store => store
            .compare_and_set_checkpoint(key, expected_version, value, ttl)
            .await)
    }

    fn is_ready(&self) -> bool {
        dispatch!(self, store => store.is_ready())
    }
}
