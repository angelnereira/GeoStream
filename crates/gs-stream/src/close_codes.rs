//! WebSocket close codes of docs/08 §8.
//!
//! The server only ever puts the code *name* in the close reason, never a detail that
//! could carry sensitive data.
#![forbid(unsafe_code)]

/// Normal closure.
pub const CLOSE_NORMAL: u16 = 1000;
/// The client navigated away or the server is closing the listener.
pub const CLOSE_GOING_AWAY: u16 = 1001;
/// Incoming frame above `GS_STREAM_MAX_FRAME_BYTES`.
pub const CLOSE_MESSAGE_TOO_BIG: u16 = 1009;
/// Malformed message, wrong frame type, message before the hello, or too many protocol errors.
pub const CLOSE_PROTOCOL_ERROR: u16 = 4000;
/// The client declared a protocol minor the server does not support.
pub const CLOSE_UNSUPPORTED_PROTOCOL: u16 = 4001;
/// No `ClientHello` before `GS_STREAM_HELLO_TIMEOUT_MS`.
pub const CLOSE_HELLO_TIMEOUT: u16 = 4002;
/// The outbound control queue is full.
pub const CLOSE_SLOW_CONSUMER: u16 = 4005;
/// Authentication expired without a refresh, or the credentials were revoked.
pub const CLOSE_AUTH_EXPIRED: u16 = 4401;
/// The tenant or application is disabled, or `AuthRefresh` came from another device.
pub const CLOSE_FORBIDDEN: u16 = 4403;
/// No frame received during `GS_STREAM_IDLE_TIMEOUT_MS`.
pub const CLOSE_IDLE_TIMEOUT: u16 = 4408;
/// Another connection of the same device took over.
pub const CLOSE_SUPERSEDED: u16 = 4409;
/// Sustained abuse of the inbound message limit.
pub const CLOSE_RATE_LIMITED: u16 = 4429;
/// Internal error.
pub const CLOSE_INTERNAL: u16 = 4500;
/// Draining or overload: reconnect after `reconnect_after_ms`.
pub const CLOSE_TRY_AGAIN: u16 = 4503;

/// Close code with the reason the server sends (docs/08 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseCode {
    /// Numeric code.
    pub code: u16,
    /// Reason sent to the client: the code name only.
    pub reason: &'static str,
}

impl CloseCode {
    /// Builds a close code, substituting a safe reason for unknown numbers.
    pub const fn new(code: u16) -> Self {
        Self {
            code,
            reason: reason_of(code),
        }
    }

    /// Name of the code, used as the close reason (docs/08 §8).
    pub const fn name(&self) -> &'static str {
        self.reason
    }
}

/// Reason of every close code of docs/08 §8.
pub const fn reason_of(code: u16) -> &'static str {
    match code {
        CLOSE_NORMAL => "NORMAL",
        CLOSE_GOING_AWAY => "GOING_AWAY",
        CLOSE_MESSAGE_TOO_BIG => "MESSAGE_TOO_BIG",
        CLOSE_PROTOCOL_ERROR => "PROTOCOL_ERROR",
        CLOSE_UNSUPPORTED_PROTOCOL => "UNSUPPORTED_PROTOCOL",
        CLOSE_HELLO_TIMEOUT => "HELLO_TIMEOUT",
        CLOSE_SLOW_CONSUMER => "SLOW_CONSUMER",
        CLOSE_AUTH_EXPIRED => "AUTH_EXPIRED",
        CLOSE_FORBIDDEN => "FORBIDDEN",
        CLOSE_IDLE_TIMEOUT => "IDLE_TIMEOUT",
        CLOSE_SUPERSEDED => "SUPERSEDED",
        CLOSE_RATE_LIMITED => "RATE_LIMITED",
        CLOSE_INTERNAL => "INTERNAL",
        CLOSE_TRY_AGAIN => "TRY_AGAIN",
        _ => "INTERNAL",
    }
}

impl From<u16> for CloseCode {
    fn from(code: u16) -> Self {
        Self::new(code)
    }
}

impl std::fmt::Display for CloseCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.code, self.reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_every_documented_close_code_has_a_name() {
        // covers: FR-STR-005, docs/08 §8
        let codes = [
            CLOSE_NORMAL,
            CLOSE_GOING_AWAY,
            CLOSE_MESSAGE_TOO_BIG,
            CLOSE_PROTOCOL_ERROR,
            CLOSE_UNSUPPORTED_PROTOCOL,
            CLOSE_HELLO_TIMEOUT,
            CLOSE_SLOW_CONSUMER,
            CLOSE_AUTH_EXPIRED,
            CLOSE_FORBIDDEN,
            CLOSE_IDLE_TIMEOUT,
            CLOSE_SUPERSEDED,
            CLOSE_RATE_LIMITED,
            CLOSE_INTERNAL,
            CLOSE_TRY_AGAIN,
        ];
        for code in codes {
            let close = CloseCode::new(code);
            assert_eq!(close.code, code);
            assert!(
                close
                    .reason
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_'),
                "reason must be the code name: {}",
                close.reason
            );
        }
    }

    #[test]
    fn test_unknown_codes_fall_back_to_internal() {
        // covers: docs/08 §8
        assert_eq!(CloseCode::new(4999).reason, "INTERNAL");
    }
}
