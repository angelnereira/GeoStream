//! Bus trait with in-memory implementation (docs/04 §6.1, ADR-0010).
//!
//! Topics carry control events, location updates and invalidation notices between instances.
//! The in-memory backend is a single instance only: subscribers of other processes do not
//! receive anything, which is the documented cost of `GS_STATE_BACKEND=memory` (ADR-0010).
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::sync::broadcast::{self, Receiver, Sender};

/// Message delivered by the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusMessage {
    /// Topic the message was published to.
    pub topic: String,
    /// Raw payload (Protobuf in production, docs/04 §6.1).
    pub payload: Vec<u8>,
}

/// Errors returned by the bus.
#[derive(Debug, Error)]
pub enum BusError {
    /// The subscription does not exist.
    #[error("unknown bus subscription {0}")]
    UnknownSubscription(u64),

    /// The connection to the backend was lost.
    #[error("bus connection lost: {0}")]
    ConnectionLost(String),

    /// The backend cannot keep up and dropped messages.
    #[error("bus subscription lagged behind: {dropped} messages were dropped")]
    Lagged { dropped: u64 },
}

/// Publisher/subscriber bus with per-topic reference counting (docs/04 §6.1).
#[allow(async_fn_in_trait)]
pub trait Bus: Send + Sync {
    /// Subscribes to a topic and returns the subscription id.
    async fn subscribe(&self, topic: &str) -> Result<u64, BusError>;

    /// Unsubscribes; returns whether the subscription existed.
    async fn unsubscribe(&self, subscription_id: u64) -> Result<bool, BusError>;

    /// Publishes a payload to every subscriber of the topic.
    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<usize, BusError>;

    /// Waits for the next message of a subscription; `None` when it is closed.
    async fn next_message(&self, subscription_id: u64) -> Result<Option<BusMessage>, BusError>;

    /// Number of live subscriptions of a topic (refcount of docs/04 §6.1).
    async fn subscription_count(&self, topic: &str) -> Result<usize, BusError>;

    /// Reports whether the connection to the backend is healthy.
    fn is_connected(&self) -> bool;
}

/// In-memory bus: one process only (ADR-0010).
#[derive(Debug, Default)]
pub struct InMemoryBus {
    senders: DashMap<String, Arc<Sender<Vec<u8>>>>,
    subscriptions: DashMap<u64, Subscription>,
    next_id: AtomicU64,
}

/// A live subscription: its topic and the receiving end of the topic channel.
#[derive(Debug)]
struct Subscription {
    topic: String,
    receiver: Arc<Mutex<Receiver<Vec<u8>>>>,
}

impl InMemoryBus {
    /// Creates a new, empty bus.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Returns the channel of a topic, creating it on first use.
    fn channel_for(&self, topic: &str) -> Arc<Sender<Vec<u8>>> {
        self.senders
            .entry(topic.to_string())
            .or_insert_with(|| {
                let (sender, _receiver) = broadcast::channel(BUS_CHANNEL_CAPACITY);
                Arc::new(sender)
            })
            .value()
            .clone()
    }
}

/// Capacity of each topic channel. Subscribers that fall behind report [`BusError::Lagged`].
const BUS_CHANNEL_CAPACITY: usize = 1_024;

impl Bus for InMemoryBus {
    async fn subscribe(&self, topic: &str) -> Result<u64, BusError> {
        let sender = self.channel_for(topic);
        let receiver = sender.subscribe();
        let subscription_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.subscriptions.insert(
            subscription_id,
            Subscription {
                topic: topic.to_string(),
                receiver: Arc::new(Mutex::new(receiver)),
            },
        );
        Ok(subscription_id)
    }

    async fn unsubscribe(&self, subscription_id: u64) -> Result<bool, BusError> {
        let removed = self.subscriptions.remove(&subscription_id);
        match removed {
            Some((_, subscription)) => {
                let still_used = self
                    .subscriptions
                    .iter()
                    .any(|entry| entry.value().topic == subscription.topic);
                if !still_used {
                    self.senders.remove(&subscription.topic);
                }
                Ok(true)
            }
            None => Err(BusError::UnknownSubscription(subscription_id)),
        }
    }

    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<usize, BusError> {
        let Some(sender) = self.senders.get(topic) else {
            return Ok(0);
        };
        // A topic without receivers is dropped: the bus is best-effort.
        Ok(sender.send(payload.to_vec()).unwrap_or(0))
    }

    async fn next_message(&self, subscription_id: u64) -> Result<Option<BusMessage>, BusError> {
        let subscription = self
            .subscriptions
            .get(&subscription_id)
            .map(|entry| entry.value().clone_subscription())
            .ok_or(BusError::UnknownSubscription(subscription_id))?;

        let mut receiver = subscription.receiver.lock().await;
        match receiver.recv().await {
            Ok(payload) => Ok(Some(BusMessage {
                topic: subscription.topic,
                payload,
            })),
            Err(broadcast::error::RecvError::Closed) => Ok(None),
            Err(broadcast::error::RecvError::Lagged(dropped)) => Err(BusError::Lagged { dropped }),
        }
    }

    async fn subscription_count(&self, topic: &str) -> Result<usize, BusError> {
        Ok(self
            .subscriptions
            .iter()
            .filter(|entry| entry.value().topic == topic)
            .count())
    }

    fn is_connected(&self) -> bool {
        true
    }
}

impl Subscription {
    /// Clones the pieces needed to receive without holding the map lock.
    fn clone_subscription(&self) -> Subscription {
        Subscription {
            topic: self.topic.clone(),
            receiver: Arc::clone(&self.receiver),
        }
    }
}

/// Convenience alias for a shared bus.
pub type SharedBus = Arc<dyn Bus>;

impl<T: Bus> Bus for Arc<T> {
    async fn subscribe(&self, topic: &str) -> Result<u64, BusError> {
        (**self).subscribe(topic).await
    }

    async fn unsubscribe(&self, subscription_id: u64) -> Result<bool, BusError> {
        (**self).unsubscribe(subscription_id).await
    }

    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<usize, BusError> {
        (**self).publish(topic, payload).await
    }

    async fn next_message(&self, subscription_id: u64) -> Result<Option<BusMessage>, BusError> {
        (**self).next_message(subscription_id).await
    }

    async fn subscription_count(&self, topic: &str) -> Result<usize, BusError> {
        (**self).subscription_count(topic).await
    }

    fn is_connected(&self) -> bool {
        (**self).is_connected()
    }
}
