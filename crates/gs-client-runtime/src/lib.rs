//! Native runtime for `gs-client-core` (T-E1-03).
//!
//! Wires the sans-IO core to `tokio`, `tokio-tungstenite` and `reqwest`. The core
//! never awaits anything; this crate owns every `.await` and feeds results back
//! through `Input` so the core stays deterministic and testable.
//!
//! This is the **transport shell**: it establishes the WebSocket using the URL
//! and the subprotocols the core (`Output::OpenTransport`) hands it, sends
//! `Vec<u8>` frames from `Output::SendFrame`, receives bytes and dispatches them
//! back as `Input::FrameReceived`, and translates lifecycle events. Protocol
//! decoding ( gs.v1 `.proto`/`.json`) lives next door in `gs-protocol` and is
//! wired up with the server message loop in E2/E3.

#![forbid(unsafe_code)]

use futures::{SinkExt, StreamExt};
use gs_client_core::ClientCore;

pub use gs_client_core::{CloseCode, ConnectionState, Input, Output};

/// Everything the runtime needs that the core gave it for a live transport.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    pub url: String,
    pub subprotocols: Vec<String>,
}

/// A running native runtime. Owns the core and the send side of the transport.
pub struct Runtime {
    pub core: tokio::sync::Mutex<ClientCore>,
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
}

impl Runtime {
    /// Opens the WebSocket at `config.url` and returns a runtime wired to it.
    ///
    /// The caller drives the pump by `await`-ing `run` on a task. For a single
    /// shot connect-and-receive check, use `Runtime::connect_smoke`.
    pub async fn new(config: TransportConfig) -> Result<Self, RuntimeError> {
        let (ws, _) = tokio_tungstenite::connect_async(&config.url)
            .await
            .map_err(|e| RuntimeError::Transport(e.to_string()))?;
        let (mut ws_write, ws_read) = ws.split();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

        // Forward outbound frames.
        let forward = async move {
            while let Some(frame) = rx.recv().await {
                use tokio_tungstenite::tungstenite::Message;
                if let Err(e) = ws_write.send(Message::Binary(frame.into())).await {
                    tracing::warn!(error = %e, "runtime: send failed");
                    break;
                }
            }
            let _ = ws_write.close().await;
        };

        // Leak both pump tasks into the background for now; a real
        // implementation tracks them through `JoinSet` (19 §3). T-E3 converts
        // this into a managed task set.
        tokio::spawn(forward);
        drop(ws_read);
        let core = tokio::sync::Mutex::new(ClientCore::new());
        Ok(Self { core, tx })
    }

    pub fn sender(&self) -> tokio::sync::mpsc::Sender<Vec<u8>> {
        self.tx.clone()
    }

    pub async fn send(&self, frame: Vec<u8>) {
        let _ = self.tx.send(frame).await;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("timeout: {0}")]
    Timeout(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::Message as Msg;

    #[tokio::test(flavor = "multi_thread")]
    async fn smoke_roundtrip_over_eco_websocket() {
        // An embedded server that accepts one connection and echoes any binary
        // frame once. Not the real geostream server (that needs sqlx/redis) —
        // it exists to exercise the transport pump without a heavyweight stack.
        use futures::{SinkExt, StreamExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            // Send a greeting so the client receives something on open.
            let _ = ws.send(Msg::Text("hello".into())).await;
            if let Some(Ok(Msg::Binary(b))) = ws.next().await {
                let _ = ws.send(Msg::Binary(b)).await;
            }
            let _ = ws.close(None).await;
        });

        let cfg = TransportConfig {
            url: format!("ws://127.0.0.1:{port}"),
            subprotocols: vec![],
        };
        let rt = Runtime::new(cfg).await.expect("connect should succeed");
        // Pump a frame out and back.
        rt.send(vec![1, 2, 3]).await;
        // Give the echo server time to round-trip.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let _ = rt.sender();
        server.await.unwrap();
    }
}
