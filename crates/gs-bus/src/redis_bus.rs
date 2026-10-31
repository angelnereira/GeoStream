//! Redis/Valkey Pub/Sub implementation of [`Bus`] (docs/04 §6.1, ADR-0010, T-F6-01).
//!
//! One RESP3 connection per instance carries every topic. The instance keeps a reference count
//! per topic: it sends `SUBSCRIBE` on the first local subscription and `UNSUBSCRIBE` after the
//! last one (docs/04 §6.1). Pushed messages are dispatched to bounded per-subscription queues;
//! a subscriber that falls behind gets [`BusError::Lagged`] instead of an unbounded backlog.
//!
//! The connection manager reconnects with backoff and re-subscribes to every live topic. Between
//! the loss and the re-subscription [`Bus::is_connected`] is `false`, so callers degrade
//! explicitly (docs/04 §7.6) instead of assuming the fan-out is healthy.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use dashmap::DashMap;
use redis::aio::{AsyncPushSender, ConnectionManager, ConnectionManagerConfig, SendError};
use redis::{IntoConnectionInfo, ProtocolVersion, PushInfo, PushKind, RedisError, Value};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use crate::{Bus, BusError, BusMessage};

/// Capacity of each subscription queue, the same as the in-memory topic channels.
const SUBSCRIPTION_QUEUE_CAPACITY: usize = 1_024;

/// Capacity of the queue between the connection and the dispatcher task.
const PUSH_QUEUE_CAPACITY: usize = 4_096;

/// Redis/Valkey bus shared by every instance (ADR-0010).
pub struct RedisBus {
    connection: ConnectionManager,
    command_timeout: Duration,
    shared: Arc<Shared>,
    /// Serializes the refcount transitions with their `SUBSCRIBE`/`UNSUBSCRIBE`, so a topic is
    /// never unsubscribed while another local subscription is being added. A `tokio` mutex is
    /// required because the guard spans the round trip to the server.
    topic_transitions: Mutex<()>,
    dispatcher: JoinHandle<()>,
}

/// Sending side of one local subscription: its id and its bounded queue.
type TopicSender = (u64, mpsc::Sender<Vec<u8>>);

/// State shared with the dispatcher task.
#[derive(Default)]
struct Shared {
    /// Senders of the live subscriptions, per topic: the refcount of docs/04 §6.1.
    topics: DashMap<String, Vec<TopicSender>>,
    subscriptions: DashMap<u64, LocalSubscription>,
    next_id: AtomicU64,
    connected: AtomicBool,
}

/// Receiving side of one local subscription.
#[derive(Clone)]
struct LocalSubscription {
    topic: String,
    receiver: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
    /// Messages dropped because the queue was full, reported once by the next receive.
    dropped: Arc<AtomicU64>,
}

impl std::fmt::Debug for RedisBus {
    // The connection carries the URL with credentials: never print it (docs/10 §8.2).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisBus")
            .field("command_timeout", &self.command_timeout)
            .field("connected", &self.shared.connected.load(Ordering::SeqCst))
            .field("topics", &self.shared.topics.len())
            .finish_non_exhaustive()
    }
}

impl RedisBus {
    /// Connects to `url` with RESP3, which carries Pub/Sub pushes on a regular connection.
    ///
    /// `command_timeout` bounds the connection and every command.
    pub async fn connect(url: &str, command_timeout: Duration) -> Result<Self, BusError> {
        let info = url
            .into_connection_info()
            .map_err(|e| connection_lost(&e))?;
        let settings = info
            .redis_settings()
            .clone()
            .set_protocol(ProtocolVersion::RESP3);
        let info = info.set_redis_settings(settings);
        let client = redis::Client::open(info).map_err(|e| connection_lost(&e))?;

        let (push_sender, push_receiver) = mpsc::channel(PUSH_QUEUE_CAPACITY);
        let shared = Arc::new(Shared::default());
        let config = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(command_timeout))
            .set_response_timeout(Some(command_timeout))
            .set_automatic_resubscription()
            .set_push_sender(PushForwarder {
                sender: push_sender,
                shared: Arc::clone(&shared),
            });
        let connection = match tokio::time::timeout(
            command_timeout,
            client.get_connection_manager_with_config(config),
        )
        .await
        {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => return Err(connection_lost(&error)),
            Err(_elapsed) => return Err(timed_out()),
        };
        shared.connected.store(true, Ordering::SeqCst);

        let dispatcher = tokio::spawn(dispatch(push_receiver, Arc::clone(&shared)));
        Ok(Self {
            connection,
            command_timeout,
            shared,
            topic_transitions: Mutex::new(()),
            dispatcher,
        })
    }

    /// Runs one command with the timeout and keeps the connection flag in sync.
    async fn run<T>(
        &self,
        operation: impl Future<Output = Result<T, RedisError>>,
    ) -> Result<T, BusError> {
        match tokio::time::timeout(self.command_timeout, operation).await {
            Ok(Ok(value)) => {
                self.shared.connected.store(true, Ordering::SeqCst);
                Ok(value)
            }
            Ok(Err(error)) => {
                if is_connection_error(&error) {
                    self.shared.connected.store(false, Ordering::SeqCst);
                }
                Err(connection_lost(&error))
            }
            Err(_elapsed) => {
                self.shared.connected.store(false, Ordering::SeqCst);
                Err(timed_out())
            }
        }
    }
}

impl Drop for RedisBus {
    fn drop(&mut self) {
        // The dispatcher only lives as long as the bus: no orphan task (docs/19 §3).
        self.dispatcher.abort();
    }
}

impl Bus for RedisBus {
    async fn subscribe(&self, topic: &str) -> Result<u64, BusError> {
        let _transition = self.topic_transitions.lock().await;
        let subscription_id = self.shared.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel(SUBSCRIPTION_QUEUE_CAPACITY);

        // Register locally before `SUBSCRIBE`: nothing published after the server acknowledges
        // the subscription can be missed.
        let first = {
            let mut senders = self.shared.topics.entry(topic.to_string()).or_default();
            senders.push((subscription_id, sender));
            senders.len() == 1
        };
        if first {
            let mut connection = self.connection.clone();
            let subscribed = self
                .run(async move { connection.subscribe(topic).await })
                .await;
            if let Err(error) = subscribed {
                self.shared.topics.remove(topic);
                return Err(error);
            }
        }

        self.shared.subscriptions.insert(
            subscription_id,
            LocalSubscription {
                topic: topic.to_string(),
                receiver: Arc::new(Mutex::new(receiver)),
                dropped: Arc::new(AtomicU64::new(0)),
            },
        );
        Ok(subscription_id)
    }

    async fn unsubscribe(&self, subscription_id: u64) -> Result<bool, BusError> {
        let _transition = self.topic_transitions.lock().await;
        let Some((_, subscription)) = self.shared.subscriptions.remove(&subscription_id) else {
            return Err(BusError::UnknownSubscription(subscription_id));
        };

        let last = match self.shared.topics.get_mut(&subscription.topic) {
            Some(mut senders) => {
                senders.retain(|(id, _)| *id != subscription_id);
                senders.is_empty()
            }
            None => false,
        };
        if last {
            self.shared.topics.remove(&subscription.topic);
            let mut connection = self.connection.clone();
            let topic = subscription.topic.as_str();
            // The local subscription is already gone. If the server cannot be told, messages of
            // the topic keep arriving and the dispatcher drops them: no subscriber can see them.
            let _unsubscribed = self
                .run(async move { connection.unsubscribe(topic).await })
                .await;
        }
        Ok(true)
    }

    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<usize, BusError> {
        let mut connection = self.connection.clone();
        // Redis counts receiving connections, i.e. instances with at least one subscriber.
        self.run(async move {
            redis::cmd("PUBLISH")
                .arg(topic)
                .arg(payload)
                .query_async::<usize>(&mut connection)
                .await
        })
        .await
    }

    async fn next_message(&self, subscription_id: u64) -> Result<Option<BusMessage>, BusError> {
        let subscription = self
            .shared
            .subscriptions
            .get(&subscription_id)
            .map(|entry| entry.value().clone())
            .ok_or(BusError::UnknownSubscription(subscription_id))?;

        let dropped = subscription.dropped.swap(0, Ordering::SeqCst);
        if dropped > 0 {
            return Err(BusError::Lagged { dropped });
        }
        let mut receiver = subscription.receiver.lock().await;
        Ok(receiver.recv().await.map(|payload| BusMessage {
            topic: subscription.topic.clone(),
            payload,
        }))
    }

    async fn subscription_count(&self, topic: &str) -> Result<usize, BusError> {
        Ok(self
            .shared
            .topics
            .get(topic)
            .map_or(0, |senders| senders.len()))
    }

    fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::SeqCst)
    }
}

/// Forwards pushes from the connection to the dispatcher without blocking the connection.
struct PushForwarder {
    sender: mpsc::Sender<PushInfo>,
    shared: Arc<Shared>,
}

impl AsyncPushSender for PushForwarder {
    fn send(&self, info: PushInfo) -> Result<(), SendError> {
        if matches!(info.kind, PushKind::Disconnection) {
            self.shared.connected.store(false, Ordering::SeqCst);
        }
        match self.sender.try_send(info) {
            Ok(()) => Ok(()),
            // A full queue loses this push only; the dispatcher keeps running.
            Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(SendError),
        }
    }
}

/// Delivers pushed messages to the local subscriptions of their topic.
async fn dispatch(mut pushes: mpsc::Receiver<PushInfo>, shared: Arc<Shared>) {
    while let Some(push) = pushes.recv().await {
        match push.kind {
            PushKind::Message => {
                let Some((topic, payload)) = message_parts(push.data) else {
                    continue;
                };
                let Some(senders) = shared.topics.get(&topic).map(|s| s.value().clone()) else {
                    continue;
                };
                for (subscription_id, sender) in senders {
                    if let Err(mpsc::error::TrySendError::Full(_)) =
                        sender.try_send(payload.clone())
                        && let Some(subscription) = shared.subscriptions.get(&subscription_id)
                    {
                        subscription.dropped.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
            PushKind::Subscribe => shared.connected.store(true, Ordering::SeqCst),
            PushKind::Disconnection => shared.connected.store(false, Ordering::SeqCst),
            _ => {}
        }
    }
}

/// Splits a `message` push into its channel and payload.
fn message_parts(data: Vec<Value>) -> Option<(String, Vec<u8>)> {
    let mut parts = data.into_iter();
    let topic = match parts.next()? {
        Value::BulkString(bytes) => String::from_utf8(bytes).ok()?,
        Value::SimpleString(text) => text,
        _ => return None,
    };
    let payload = match parts.next()? {
        Value::BulkString(bytes) => bytes,
        _ => return None,
    };
    Some((topic, payload))
}

/// Whether the error means the backend is unreachable.
fn is_connection_error(error: &RedisError) -> bool {
    error.is_io_error()
        || error.is_timeout()
        || error.is_connection_dropped()
        || error.is_connection_refusal()
}

/// Maps an error without its details: they may echo the URL (docs/10 §8.2).
fn connection_lost(error: &RedisError) -> BusError {
    BusError::ConnectionLost(format!("{:?}", error.kind()))
}

fn timed_out() -> BusError {
    BusError::ConnectionLost("command timed out (GS_REDIS_COMMAND_TIMEOUT_MS)".to_string())
}
