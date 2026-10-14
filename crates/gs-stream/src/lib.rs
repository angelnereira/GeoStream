//! Stream connection: WebSocket upgrade handling, hello handshake, state machine,
//! heartbeat and close codes (docs/08 §2–§6.5 and §8, FR-STR-001, FR-STR-005).
//!
//! The crate only owns the connection. The upgrade *route* and its Problem Details
//! rejections live in `gs-api` (docs/04 §4.1: `gs-stream` cannot depend on `gs-api`), so the
//! binary wires both: `gs-api` runs the pre-upgrade checks of docs/08 §3 and hands the
//! accepted socket to [`run_session`].
#![forbid(unsafe_code)]

pub mod close_codes;
pub mod codec;
pub mod config;
pub mod connection;
pub mod subscription;

pub use close_codes::{CLOSE_AUTH_EXPIRED, CloseCode, reason_of};
pub use codec::{FrameCodec, FrameError};
pub use config::{StreamConfig, StreamConfigError, StreamLimits, TenantStreamLimits};
pub use connection::{
    AcceptedSession, Connection, ConnectionOutcome, ConnectionState, MAX_PROTOCOL_MINOR,
    StreamState, run_session,
};
pub use subscription::{
    Subscription, SubscriptionSet, SubscriptionState, subscribe_ack, unsubscribe_ack,
};
