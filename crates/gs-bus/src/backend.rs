//! Runtime selection of the bus backend (`GS_STATE_BACKEND`, docs/17 §2.3, ADR-0010).
//!
//! The trait uses `async fn`, so it cannot be a trait object. An enum keeps the futures
//! concrete (and therefore provably `Send` for the stream tasks) while `serve` chooses the
//! backend from the configuration.

use std::sync::Arc;

use crate::{Bus, BusError, BusMessage, InMemoryBus, RedisBus};

/// The bus a process uses: in memory (one instance) or Redis/Valkey Pub/Sub (shared).
#[derive(Debug, Clone)]
pub enum BusBackend {
    /// `GS_STATE_BACKEND=memory`: a single instance only.
    Memory(Arc<InMemoryBus>),
    /// `GS_STATE_BACKEND=redis`: messages reach every instance.
    Redis(Arc<RedisBus>),
}

impl BusBackend {
    /// A new in-memory backend.
    pub fn memory() -> Arc<Self> {
        Arc::new(Self::Memory(InMemoryBus::new()))
    }

    /// Wraps an existing in-memory bus, sharing its subscriptions.
    pub fn from_memory(bus: Arc<InMemoryBus>) -> Arc<Self> {
        Arc::new(Self::Memory(bus))
    }

    /// Actively checks that the backend answers: `PING` for Redis/Valkey, always `Ok` in memory.
    pub async fn ping(&self) -> Result<(), BusError> {
        match self {
            Self::Memory(_) => Ok(()),
            Self::Redis(bus) => bus.ping().await,
        }
    }

    /// Whether this backend reaches other instances (`GS_STATE_BACKEND=redis`).
    pub fn is_shared(&self) -> bool {
        matches!(self, Self::Redis(_))
    }

    /// Wraps a connected Redis/Valkey bus.
    pub fn from_redis(bus: RedisBus) -> Arc<Self> {
        Arc::new(Self::Redis(Arc::new(bus)))
    }
}

/// Forwards one trait call to the selected backend.
macro_rules! dispatch {
    ($self:ident, $bus:ident => $call:expr) => {
        match $self {
            BusBackend::Memory($bus) => $call,
            BusBackend::Redis($bus) => $call,
        }
    };
}

impl Bus for BusBackend {
    async fn subscribe(&self, topic: &str) -> Result<u64, BusError> {
        dispatch!(self, bus => bus.subscribe(topic).await)
    }

    async fn unsubscribe(&self, subscription_id: u64) -> Result<bool, BusError> {
        dispatch!(self, bus => bus.unsubscribe(subscription_id).await)
    }

    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<usize, BusError> {
        dispatch!(self, bus => bus.publish(topic, payload).await)
    }

    async fn next_message(&self, subscription_id: u64) -> Result<Option<BusMessage>, BusError> {
        dispatch!(self, bus => bus.next_message(subscription_id).await)
    }

    async fn subscription_count(&self, topic: &str) -> Result<usize, BusError> {
        dispatch!(self, bus => bus.subscription_count(topic).await)
    }

    fn is_connected(&self) -> bool {
        dispatch!(self, bus => bus.is_connected())
    }
}
