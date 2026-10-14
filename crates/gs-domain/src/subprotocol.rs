//! Negotiated subprotocol of the stream transport (docs/08 §2).
//!
//! The names and the negotiation rule are pure: no I/O, no clock, no state.
#![forbid(unsafe_code)]

/// Subprotocol of the binary transport.
pub const SUBPROTOCOL_PROTO: &str = "gs.v1.proto";
/// Subprotocol of the JSON transport, only offered when enabled (docs/17 §3).
pub const SUBPROTOCOL_JSON: &str = "gs.v1.json";

/// Selected subprotocol of a connection (docs/08 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subprotocol {
    /// `gs.v1.proto`: Protobuf, binary frames.
    Proto,
    /// `gs.v1.json`: canonical proto3 JSON, text frames.
    Json,
}

impl Subprotocol {
    /// Header value the server answers with (docs/08 §2).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proto => SUBPROTOCOL_PROTO,
            Self::Json => SUBPROTOCOL_JSON,
        }
    }

    /// Whether the frames of this subprotocol are text frames (docs/08 §2).
    pub const fn is_text(self) -> bool {
        matches!(self, Self::Json)
    }
}

impl std::fmt::Display for Subprotocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Negotiates the subprotocol: the first one offered by the client that the server supports
/// and has enabled (docs/08 §2).
///
/// Returns `None` when no offered subprotocol is usable, which the upgrade answers with
/// HTTP 400 `PROTOCOL_VERSION_UNSUPPORTED` (docs/08 §3).
pub fn negotiate(offered: Option<&str>, json_enabled: bool) -> Option<Subprotocol> {
    let offered = offered?;
    for candidate in offered.split(',').map(str::trim) {
        if candidate.eq_ignore_ascii_case(SUBPROTOCOL_PROTO) {
            return Some(Subprotocol::Proto);
        }
        if json_enabled && candidate.eq_ignore_ascii_case(SUBPROTOCOL_JSON) {
            return Some(Subprotocol::Json);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_is_always_offered_and_preferred() {
        // covers: docs/08 §2
        assert_eq!(
            negotiate(Some("gs.v1.proto"), false),
            Some(Subprotocol::Proto)
        );
        assert_eq!(
            negotiate(Some("gs.v1.json, gs.v1.proto"), true),
            Some(Subprotocol::Json),
            "the first offered subprotocol the server supports wins"
        );
        assert_eq!(
            negotiate(Some("gs.v1.json"), false),
            None,
            "the JSON subprotocol must not be offered when disabled"
        );
        assert_eq!(
            negotiate(Some("gs.v1.proto, gs.v1.json"), true),
            Some(Subprotocol::Proto)
        );
        assert_eq!(negotiate(Some("other.v1"), true), None);
        assert_eq!(negotiate(None, true), None);
        assert_eq!(negotiate(Some(""), true), None);
    }

    #[test]
    fn test_frame_type_matches_the_subprotocol() {
        // covers: docs/08 §2
        assert!(!Subprotocol::Proto.is_text());
        assert!(Subprotocol::Json.is_text());
        assert_eq!(Subprotocol::Proto.as_str(), "gs.v1.proto");
        assert_eq!(Subprotocol::Json.to_string(), "gs.v1.json");
    }
}
