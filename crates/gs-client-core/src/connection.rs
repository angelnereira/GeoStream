//! Connection state machine and backoff of `12` §8.
//!
//! Atomic units:
//!   `disconnected ⇢ connecting ⇢ connected ⇢ disconnected`
//! with an explicit `reconnecting` phase that names the delay. The
//! **transitions** below are the only ways state can change; the rest of the
//! crate (`ClientCore`) is responsible for emitting `Output`s.

#![forbid(unsafe_code)]

use crate::ports::{ClientEvent, CloseCode, ConnectionState, ReconnectAction, ReconnectPolicy};

#[derive(Debug, Clone)]
pub struct Connection {
    pub state: ConnectionState,
    pub reconnect: ReconnectPolicy,
    /// Consecutive failures since the counter was last reset.
    pub failures: u32,
    /// When the connection became stable (for the 60 s reset rule).
    pub stable_since_ms: Option<u64>,
    /// `ServerNotice.reconnect_after_ms` remembered for a pending
    /// `4503 TRY_AGAIN` reconnect. `None` when no notice was seen.
    pub notice_reconnect_after_ms: Option<u64>,
}

impl Connection {
    pub fn new() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            reconnect: ReconnectPolicy::new(),
            failures: 0,
            stable_since_ms: None,
            notice_reconnect_after_ms: None,
        }
    }

    pub fn begin_connect(&mut self) {
        if self.state != ConnectionState::Disconnected {
            return;
        }
        self.state = ConnectionState::Connecting;
    }

    pub fn on_opened(&mut self, now_ms: u64) -> Vec<ClientEvent> {
        self.state = ConnectionState::Connected;
        self.reconnect.note_connected(now_ms);
        self.stable_since_ms = Some(now_ms);
        self.failures = 0;
        self.notice_reconnect_after_ms = None;
        vec![ClientEvent::ConnectionChanged(ConnectionState::Connected)]
    }

    pub fn on_closed(
        &mut self,
        code: CloseCode,
        now_ms: u64,
    ) -> (ReconnectAction, Vec<ClientEvent>, u64) {
        let action = crate::ports::close_action(code, self.notice_reconnect_after_ms);
        // Reset the notice regardless: a single notice never carries across reconnect.
        self.notice_reconnect_after_ms = None;

        match action {
            ReconnectAction::Stop => {
                self.state = ConnectionState::Disconnected;
            }
            ReconnectAction::Immediate => {
                self.state = ConnectionState::Reconnecting;
            }
            ReconnectAction::RefreshToken => {
                self.state = ConnectionState::Disconnected;
            }
            ReconnectAction::RateLimited => {
                self.state = ConnectionState::Reconnecting;
            }
            ReconnectAction::After(_) | ReconnectAction::Backoff => {
                self.state = ConnectionState::Reconnecting;
            }
        }
        // Backoff counter: only real failures that use backoff increment it.
        if matches!(action, ReconnectAction::Backoff) {
            self.reconnect.note_failed();
            self.failures += 1;
        }
        // Stable for >= 60 s? The caller invokes reset_stable_counter when a
        // connection lasts that long; we keep the field so `next_timer_ms` can
        // decide when to re-evaluate.
        let stable = self.stable_since_ms.map(|s| now_ms.saturating_sub(s));
        let reset_due = stable
            .map(|s| s >= ReconnectPolicy::STABLE_RESET_MS)
            .unwrap_or(false);
        if reset_due {
            self.reconnect.attempt = 0;
            self.failures = 0;
            self.stable_since_ms = None;
        }
        let mut events = vec![ClientEvent::ConnectionChanged(self.state)];
        if matches!(
            action,
            ReconnectAction::Immediate | ReconnectAction::RefreshToken | ReconnectAction::Backoff
        ) {
            // no-op: emit code separately? Keep it simple.
        }
        // Keep the unused to silence the lint on now_ms consumers:
        let _ = now_ms;
        events.push(ClientEvent::Reconnecting {
            state: self.state,
            code,
        });
        let delay = match action {
            ReconnectAction::Immediate => 0,
            ReconnectAction::After(ms) => ms,
            ReconnectAction::RateLimited => 60_000,
            ReconnectAction::RefreshToken => 0,
            ReconnectAction::Backoff => self.reconnect.delay_ms(0.0),
            ReconnectAction::Stop => 0,
        };
        (action, events, delay)
    }

    pub fn set_notice_reconnect_after_ms(&mut self, ms: u64) {
        self.notice_reconnect_after_ms = Some(ms);
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }
}

impl Default for Connection {
    fn default() -> Self {
        Self::new()
    }
}
