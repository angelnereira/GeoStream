//! Test WebSocket client for stream scenarios (docs/18 §6, contracts/README §5.3).
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use gs_domain::Subprotocol;
use gs_protocol::ServerMessage;
use gs_protocol::client_message::Body as ClientBody;
use gs_protocol::server_message::Body as ServerBody;
use prost::Message as ProstMessage;
use serde_json::Value;
use tokio::sync::{Mutex, Notify, oneshot};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use crate::matcher::{normalize_json_keys, partial_match};

/// Error produced by stream client interactions.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("WebSocket connection failed: {0}")]
    Connect(String),
    #[error("WebSocket send error: {0}")]
    Send(String),
    #[error("Failed to decode server message: {0}")]
    Decode(String),
    #[error("Timeout waiting for expected reply (request_id={0})")]
    ReplyTimeout(u64),
    #[error("Timeout waiting for unsolicited message ({0})")]
    ExpectTimeout(String),
    #[error("Unexpected message arrived during expect_none: {0:?}")]
    UnexpectedMessage(Value),
    #[error("Timeout waiting for connection close (expected code {0})")]
    CloseTimeout(u16),
    #[error("Connection closed with unexpected code: {0:?}")]
    WrongCloseCode(Option<u16>),
}

/// A connected stream test client.
pub struct TestStreamClient {
    subprotocol: Subprotocol,
    sink: Arc<
        Mutex<
            futures::stream::SplitSink<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                >,
                WsMessage,
            >,
        >,
    >,
    next_request_id: Arc<Mutex<u64>>,
    pending_replies: Arc<Mutex<HashMap<u64, oneshot::Sender<ServerMessage>>>>,
    inbox: Arc<Mutex<Vec<ServerMessage>>>,
    inbox_notify: Arc<Notify>,
    close_code: Arc<Mutex<Option<u16>>>,
    close_notify: Arc<Notify>,
    _reader_task: tokio::task::JoinHandle<()>,
}

impl TestStreamClient {
    /// Connects to a WebSocket stream endpoint using the specified subprotocol.
    pub async fn connect(ws_url: &str, subprotocol: Subprotocol) -> Result<Self, ClientError> {
        let mut request = ws_url
            .into_client_request()
            .map_err(|e| ClientError::Connect(e.to_string()))?;
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            HeaderValue::from_str(subprotocol.as_str())
                .map_err(|e| ClientError::Connect(e.to_string()))?,
        );

        let (ws_stream, response) = connect_async(request)
            .await
            .map_err(|e| ClientError::Connect(e.to_string()))?;

        if response.status() != 101 {
            return Err(ClientError::Connect(format!(
                "Unexpected HTTP upgrade status: {}",
                response.status()
            )));
        }

        let (sink, mut stream) = ws_stream.split();
        let sink = Arc::new(Mutex::new(sink));
        let next_request_id = Arc::new(Mutex::new(1u64));
        let pending_replies: Arc<Mutex<HashMap<u64, oneshot::Sender<ServerMessage>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let inbox = Arc::new(Mutex::new(Vec::new()));
        let inbox_notify = Arc::new(Notify::new());
        let close_code = Arc::new(Mutex::new(None));
        let close_notify = Arc::new(Notify::new());

        let reader_subprotocol = subprotocol;
        let reader_replies = Arc::clone(&pending_replies);
        let reader_inbox = Arc::clone(&inbox);
        let reader_inbox_notify = Arc::clone(&inbox_notify);
        let reader_close = Arc::clone(&close_code);
        let reader_close_notify = Arc::clone(&close_notify);

        let reader_task = tokio::spawn(async move {
            while let Some(msg_res) = stream.next().await {
                match msg_res {
                    Ok(WsMessage::Text(text)) => {
                        match serde_json::from_str::<ServerMessage>(&text) {
                            Ok(server_msg) => {
                                dispatch_message(
                                    server_msg,
                                    &reader_replies,
                                    &reader_inbox,
                                    &reader_inbox_notify,
                                )
                                .await;
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to deserialize text ServerMessage: {e}, text: {text}"
                                );
                            }
                        }
                    }
                    Ok(WsMessage::Binary(bytes)) => {
                        let decoded = match reader_subprotocol {
                            Subprotocol::Proto => {
                                ServerMessage::decode(&bytes[..]).map_err(|e| e.to_string())
                            }
                            Subprotocol::Json => {
                                serde_json::from_slice(&bytes[..]).map_err(|e| e.to_string())
                            }
                        };
                        match decoded {
                            Ok(server_msg) => {
                                dispatch_message(
                                    server_msg,
                                    &reader_replies,
                                    &reader_inbox,
                                    &reader_inbox_notify,
                                )
                                .await;
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to decode binary ServerMessage: {e}, len: {}",
                                    bytes.len()
                                );
                            }
                        }
                    }
                    Ok(WsMessage::Close(frame)) => {
                        let code = frame.map(|f| u16::from(f.code)).unwrap_or(1005);
                        *reader_close.lock().await = Some(code);
                        reader_close_notify.notify_waiters();
                        break;
                    }
                    Err(e) => {
                        if let tokio_tungstenite::tungstenite::Error::ConnectionClosed = e
                            && reader_close.lock().await.is_none()
                        {
                            *reader_close.lock().await = Some(1000);
                        }
                        reader_close_notify.notify_waiters();
                        break;
                    }
                    _ => {}
                }
            }
            reader_close_notify.notify_waiters();
        });

        Ok(Self {
            subprotocol,
            sink,
            next_request_id,
            pending_replies,
            inbox,
            inbox_notify,
            close_code,
            close_notify,
            _reader_task: reader_task,
        })
    }

    /// Allocates the next request ID.
    pub async fn next_id(&self) -> u64 {
        let mut id = self.next_request_id.lock().await;
        let cur = *id;
        *id += 1;
        cur
    }

    /// Sends a ClientMessage with body and waits for a reply with matching request_id.
    pub async fn send_with_reply(
        &self,
        request_id: u64,
        body: ClientBody,
        timeout: Duration,
    ) -> Result<ServerMessage, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.pending_replies.lock().await.insert(request_id, tx);

        self.send_client_message(gs_protocol::ClientMessage {
            request_id,
            body: Some(body),
        })
        .await?;

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(msg)) => Ok(msg),
            _ => {
                self.pending_replies.lock().await.remove(&request_id);
                Err(ClientError::ReplyTimeout(request_id))
            }
        }
    }

    /// Sends a ClientMessage without awaiting a correlated reply.
    pub async fn send_fire_and_forget(
        &self,
        request_id: u64,
        body: ClientBody,
    ) -> Result<(), ClientError> {
        self.send_client_message(gs_protocol::ClientMessage {
            request_id,
            body: Some(body),
        })
        .await
    }

    /// Encodes and transmits a ClientMessage over the WebSocket.
    pub async fn send_client_message(
        &self,
        msg: gs_protocol::ClientMessage,
    ) -> Result<(), ClientError> {
        let ws_msg = match self.subprotocol {
            Subprotocol::Proto => WsMessage::Binary(msg.encode_to_vec().into()),
            Subprotocol::Json => {
                let json_str =
                    serde_json::to_string(&msg).map_err(|e| ClientError::Send(e.to_string()))?;
                WsMessage::Text(json_str.into())
            }
        };

        let mut sink = self.sink.lock().await;
        sink.send(ws_msg)
            .await
            .map_err(|e| ClientError::Send(e.to_string()))
    }

    /// Waits for an unsolicited message matching `variant_name` and optional `match_val`.
    pub async fn expect_unsolicited(
        &self,
        variant_name: &str,
        match_val: Option<&Value>,
        timeout: Duration,
    ) -> Result<ServerMessage, ClientError> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            {
                let mut inbox = self.inbox.lock().await;
                let mut match_idx = None;
                for (idx, msg) in inbox.iter().enumerate() {
                    if server_message_matches(msg, variant_name, match_val) {
                        match_idx = Some(idx);
                        break;
                    }
                }
                if let Some(idx) = match_idx {
                    return Ok(inbox.remove(idx));
                }
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                let current_inbox = self.inbox.lock().await;
                tracing::debug!(
                    "expect_unsolicited timeout for variant '{variant_name}', inbox size: {}",
                    current_inbox.len()
                );
                return Err(ClientError::ExpectTimeout(variant_name.to_string()));
            }

            let wait_time = deadline - now;
            let _ = tokio::time::timeout(wait_time, self.inbox_notify.notified()).await;
        }
    }

    /// Receives the next incoming unsolicited ServerMessage from the inbox, or waits up to `timeout`.
    /// Returns `Ok(None)` if the timeout expires or the connection closes.
    pub async fn next_message(
        &self,
        timeout: Duration,
    ) -> Result<Option<ServerMessage>, ClientError> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            {
                let mut inbox = self.inbox.lock().await;
                if !inbox.is_empty() {
                    return Ok(Some(inbox.remove(0)));
                }
            }

            if self.close_code.lock().await.is_some() {
                return Ok(None);
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Ok(None);
            }

            let wait_time = deadline - now;
            let _ = tokio::time::timeout(wait_time, self.inbox_notify.notified()).await;
        }
    }

    /// Verifies that NO message matching `variant_name` arrives during `duration`.
    pub async fn expect_none(
        &self,
        variant_name: &str,
        match_val: Option<&Value>,
        duration: Duration,
    ) -> Result<(), ClientError> {
        let deadline = tokio::time::Instant::now() + duration;

        loop {
            {
                let inbox = self.inbox.lock().await;
                for msg in inbox.iter() {
                    if server_message_matches(msg, variant_name, match_val) {
                        let json = serde_json::to_value(msg).unwrap_or(Value::Null);
                        return Err(ClientError::UnexpectedMessage(json));
                    }
                }
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Ok(());
            }

            let wait_time = deadline - now;
            let _ = tokio::time::timeout(wait_time, self.inbox_notify.notified()).await;
        }
    }

    /// Waits up to `timeout` for the connection to be closed with `expected_code`.
    pub async fn expect_close(
        &self,
        expected_code: u16,
        timeout: Duration,
    ) -> Result<(), ClientError> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if let Some(code) = *self.close_code.lock().await {
                if code == expected_code {
                    return Ok(());
                } else {
                    return Err(ClientError::WrongCloseCode(Some(code)));
                }
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Err(ClientError::CloseTimeout(expected_code));
            }

            let wait_time = deadline - now;
            let _ = tokio::time::timeout(wait_time, self.close_notify.notified()).await;
        }
    }

    /// Closes the WebSocket connection cleanly (code 1000).
    pub async fn close(&self) -> Result<(), ClientError> {
        let mut sink = self.sink.lock().await;
        sink.close()
            .await
            .map_err(|e| ClientError::Send(e.to_string()))
    }
}

async fn dispatch_message(
    msg: ServerMessage,
    replies: &Arc<Mutex<HashMap<u64, oneshot::Sender<ServerMessage>>>>,
    inbox: &Arc<Mutex<Vec<ServerMessage>>>,
    inbox_notify: &Arc<Notify>,
) {
    if msg.request_id > 0 {
        let mut map = replies.lock().await;
        if let Some(tx) = map.remove(&msg.request_id) {
            let _ = tx.send(msg);
            return;
        }
    }

    let mut q = inbox.lock().await;
    if let Some(ServerBody::SubscriptionEnded(ref ended)) = msg.body {
        q.retain(|m| {
            if let Some(ServerBody::LocationUpdate(ref loc)) = m.body {
                loc.session_id != ended.session_id
            } else {
                true
            }
        });
    }
    q.push(msg);
    inbox_notify.notify_waiters();
}

/// Checks if a ServerMessage matches a variant name and optional pattern.
pub fn server_message_matches(
    msg: &ServerMessage,
    variant_name: &str,
    match_val: Option<&Value>,
) -> bool {
    let body = match &msg.body {
        Some(b) => b,
        None => return false,
    };

    let actual_name = match body {
        ServerBody::Hello(_) => "hello",
        ServerBody::SubscribeAck(_) => "subscribe_ack",
        ServerBody::UnsubscribeAck(_) => "unsubscribe_ack",
        ServerBody::PublishAck(_) => "publish_ack",
        ServerBody::HeartbeatAck(_) => "heartbeat_ack",
        ServerBody::AuthRefreshAck(_) => "auth_refresh_ack",
        ServerBody::LocationUpdate(_) => "location_update",
        ServerBody::MemberStatus(_) => "member_status",
        ServerBody::MemberEvent(_) => "member_event",
        ServerBody::SessionEvent(_) => "session_event",
        ServerBody::SubscriptionEnded(_) => "subscription_ended",
        ServerBody::AuthExpiring(_) => "auth_expiring",
        ServerBody::ServerNotice(_) => "server_notice",
        ServerBody::GeofenceEvent(_) => "geofence_event",
        ServerBody::Error(_) => "error",
    };

    if actual_name != variant_name && !(actual_name == "hello" && variant_name == "hello_ack") {
        return false;
    }

    if let Some(expected_pattern) = match_val {
        let json_val = match serde_json::to_value(msg) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let norm = normalize_json_keys(&json_val);
        if let Some(inner) = norm.get(actual_name) {
            return partial_match(expected_pattern, inner);
        }
        return partial_match(expected_pattern, &norm);
    }

    true
}
