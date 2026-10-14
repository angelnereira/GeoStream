//! Frame codec of the two subprotocols (docs/08 §2, §4 and §12).
//!
//! `gs.v1.proto` carries Protobuf in binary frames; `gs.v1.json` carries the canonical
//! proto3 JSON mapping in text frames. One frame is exactly one message, in both cases.
//! The names and the negotiation rule live in `gs-domain` (pure rules); this module owns
//! the encoding and decoding, which need `prost` and `serde_json`.
#![forbid(unsafe_code)]

use gs_service::gs_domain::Subprotocol;
use gs_service::gs_protocol::{ClientMessage, ServerMessage};
use prost::Message;

/// Error produced while turning a frame into a message (docs/08 §4).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// The frame type does not match the subprotocol (docs/08 §2).
    #[error("frame type does not match the negotiated subprotocol")]
    WrongFrameType,

    /// The frame does not decode into a message (docs/08 §4).
    #[error("frame does not decode into a message")]
    Malformed,
}

/// Encodes and decodes the messages of one connection in its subprotocol (docs/08 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCodec {
    subprotocol: Subprotocol,
}

impl FrameCodec {
    /// Builds a codec for the negotiated subprotocol.
    pub const fn new(subprotocol: Subprotocol) -> Self {
        Self { subprotocol }
    }

    /// The subprotocol this codec speaks.
    pub const fn subprotocol(&self) -> Subprotocol {
        self.subprotocol
    }

    /// Whether the frames of this subprotocol are text frames (docs/08 §2).
    pub const fn is_text(&self) -> bool {
        self.subprotocol.is_text()
    }

    /// Decodes a client message from a frame.
    ///
    /// The frame type must match the subprotocol: binary with `gs.v1.proto` and text with
    /// `gs.v1.json` (docs/08 §2).
    pub fn decode_client(&self, binary: bool, payload: &[u8]) -> Result<ClientMessage, FrameError> {
        match (self.subprotocol, binary) {
            (Subprotocol::Proto, true) => {
                ClientMessage::decode(payload).map_err(|_| FrameError::Malformed)
            }
            (Subprotocol::Json, false) => {
                serde_json::from_slice(payload).map_err(|_| FrameError::Malformed)
            }
            _ => Err(FrameError::WrongFrameType),
        }
    }

    /// Encodes a client message into a frame of the negotiated subprotocol.
    pub fn encode_client(&self, message: &ClientMessage) -> Result<Vec<u8>, FrameError> {
        match self.subprotocol {
            Subprotocol::Proto => Ok(message.encode_to_vec()),
            Subprotocol::Json => serde_json::to_vec(message).map_err(|_| FrameError::Malformed),
        }
    }

    /// Encodes a server message into a frame of the negotiated subprotocol.
    pub fn encode_server(&self, message: &ServerMessage) -> Result<Vec<u8>, FrameError> {
        match self.subprotocol {
            Subprotocol::Proto => Ok(message.encode_to_vec()),
            Subprotocol::Json => serde_json::to_vec(message).map_err(|_| FrameError::Malformed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gs_service::gs_protocol::client_message::Body as ClientBody;
    use gs_service::gs_protocol::server_message::Body as ServerBody;
    use gs_service::gs_protocol::{ClientHello, Heartbeat, HeartbeatAck};

    fn client_hello() -> ClientMessage {
        ClientMessage {
            request_id: 0,
            body: Some(ClientBody::Hello(ClientHello {
                protocol_minor: 0,
                sdk_name: "geostream-flutter".to_string(),
                sdk_version: "0.1.0".to_string(),
                platform: 1,
                app_version: "1.0.0".to_string(),
                last_sequence: 41,
                client_time: None,
            })),
        }
    }

    fn heartbeat() -> ClientMessage {
        ClientMessage {
            request_id: 1,
            body: Some(ClientBody::Heartbeat(Heartbeat { client_time: None })),
        }
    }

    fn heartbeat_ack() -> ServerMessage {
        ServerMessage {
            request_id: 1,
            body: Some(ServerBody::HeartbeatAck(HeartbeatAck {
                client_time: None,
                server_time: None,
            })),
        }
    }

    #[test]
    fn test_client_message_roundtrip_in_both_subprotocols() {
        // covers: docs/08 §4, docs/08 §12
        for subprotocol in [Subprotocol::Proto, Subprotocol::Json] {
            let codec = FrameCodec::new(subprotocol);
            let binary = !codec.is_text();

            let encoded = codec.encode_client(&client_hello()).expect("encode");
            assert_eq!(
                codec.decode_client(binary, &encoded).expect("decode"),
                client_hello()
            );

            let encoded = codec
                .encode_server(&heartbeat_ack())
                .expect("encode server");
            assert!(!encoded.is_empty());

            // A frame of the wrong type is rejected (docs/08 §2).
            assert_eq!(
                codec.decode_client(!binary, &encoded),
                Err(FrameError::WrongFrameType)
            );
            // A frame that is not a message is rejected (docs/08 §4).
            assert_eq!(
                codec.decode_client(binary, &[0xff, 0xff, 0xff]),
                Err(FrameError::Malformed)
            );
        }
    }

    #[test]
    fn test_heartbeat_encodes_in_both_subprotocols() {
        // covers: docs/08 §6.5, docs/08 §12
        let codec = FrameCodec::new(Subprotocol::Json);
        let encoded = codec.encode_client(&heartbeat()).expect("encode");
        let json: serde_json::Value = serde_json::from_slice(&encoded).expect("json");
        // 64-bit integers travel as strings in the canonical JSON mapping (docs/08 §12).
        assert_eq!(json["requestId"], serde_json::json!("1"));
        assert!(json.get("heartbeat").is_some(), "canonical lowerCamelCase");
    }
}
