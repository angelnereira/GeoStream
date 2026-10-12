//! Roundtrip tests for stream messages (T-F0-04).
//! JSON -> Protobuf -> Binary -> Protobuf -> JSON.

use prost::Message;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct TestVectors {
    #[allow(dead_code)]
    description: String,
    #[allow(dead_code)]
    version: u32,
    messages: Vec<TestMessage>,
}

#[derive(Debug, Deserialize)]
struct TestMessage {
    id: String,
    #[serde(rename = "type")]
    msg_type: String,
    description: String,
    json: Value,
}

#[test]
fn test_stream_messages_roundtrip() {
    // covers: T-F0-04
    let vector_data = include_str!("../../../contracts/test-vectors/stream_messages.json");
    let vectors: TestVectors =
        serde_json::from_str(vector_data).expect("failed to parse stream_messages.json");

    assert_eq!(
        vectors.messages.len(),
        24,
        "expected exactly 24 stream messages"
    );

    for msg in &vectors.messages {
        match msg.msg_type.as_str() {
            "ClientMessage" => {
                // 1. JSON -> Protobuf struct
                let parsed: gs_protocol::ClientMessage = serde_json::from_value(msg.json.clone())
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to parse ClientMessage {} ({}): {e}",
                            msg.id, msg.description
                        )
                    });

                // 2. Protobuf -> Binary
                let bytes = parsed.encode_to_vec();

                // 3. Binary -> Protobuf struct
                let decoded =
                    gs_protocol::ClientMessage::decode(bytes.as_slice()).unwrap_or_else(|e| {
                        panic!(
                            "Failed to decode binary for ClientMessage {} ({}): {e}",
                            msg.id, msg.description
                        )
                    });

                assert_eq!(
                    parsed, decoded,
                    "Decoded message does not match parsed message for {}",
                    msg.id
                );

                // 4. Protobuf -> JSON
                let mut reserialized = serde_json::to_value(&decoded).unwrap_or_else(|e| {
                    panic!("Failed to serialize ClientMessage {} to JSON: {e}", msg.id)
                });
                normalize_timestamps(&mut reserialized);

                assert_json_semantic_eq(
                    &msg.json,
                    &reserialized,
                    &format!("{}: ClientMessage", msg.id),
                );
            }
            "ServerMessage" => {
                // 1. JSON -> Protobuf struct
                let parsed: gs_protocol::ServerMessage = serde_json::from_value(msg.json.clone())
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to parse ServerMessage {} ({}): {e}",
                            msg.id, msg.description
                        )
                    });

                // 2. Protobuf -> Binary
                let bytes = parsed.encode_to_vec();

                // 3. Binary -> Protobuf struct
                let decoded =
                    gs_protocol::ServerMessage::decode(bytes.as_slice()).unwrap_or_else(|e| {
                        panic!(
                            "Failed to decode binary for ServerMessage {} ({}): {e}",
                            msg.id, msg.description
                        )
                    });

                assert_eq!(
                    parsed, decoded,
                    "Decoded message does not match parsed message for {}",
                    msg.id
                );

                // 4. Protobuf -> JSON
                let mut reserialized = serde_json::to_value(&decoded).unwrap_or_else(|e| {
                    panic!("Failed to serialize ServerMessage {} to JSON: {e}", msg.id)
                });
                normalize_timestamps(&mut reserialized);

                assert_json_semantic_eq(
                    &msg.json,
                    &reserialized,
                    &format!("{}: ServerMessage", msg.id),
                );
            }
            other => panic!("Unknown message type: {other} in {}", msg.id),
        }
    }
}

fn assert_json_semantic_eq(left: &Value, right: &Value, path: &str) {
    match (left, right) {
        (Value::Object(l), Value::Object(r)) => {
            assert_eq!(
                l.len(),
                r.len(),
                "Object key count mismatch at {path}: left={:?}, right={:?}",
                l.keys(),
                r.keys()
            );
            for (k, v_left) in l {
                let v_right = r
                    .get(k)
                    .unwrap_or_else(|| panic!("Missing key '{k}' in right at {path}"));
                assert_json_semantic_eq(v_left, v_right, &format!("{path}.{k}"));
            }
        }
        (Value::Array(l), Value::Array(r)) => {
            assert_eq!(l.len(), r.len(), "Array length mismatch at {path}");
            for (i, (v_left, v_right)) in l.iter().zip(r.iter()).enumerate() {
                assert_json_semantic_eq(v_left, v_right, &format!("{path}[{i}]"));
            }
        }
        (Value::Number(l), Value::Number(r)) => {
            let lf = l.as_f64().expect("left number as f64");
            let rf = r.as_f64().expect("right number as f64");
            assert!(
                (lf - rf).abs() < 1e-9,
                "Number mismatch at {path}: {lf} vs {rf}"
            );
        }
        (Value::String(l), Value::String(r)) => {
            assert_eq!(l, r, "String mismatch at {path}");
        }
        (Value::Bool(l), Value::Bool(r)) => {
            assert_eq!(l, r, "Bool mismatch at {path}");
        }
        (Value::Null, Value::Null) => {}
        _ => panic!("Type mismatch at {path}: left={left:?}, right={right:?}"),
    }
}

fn normalize_timestamps(val: &mut Value) {
    match val {
        Value::String(s) => {
            if s.ends_with("+00:00") {
                *s = format!("{}Z", &s[..s.len() - 6]);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                normalize_timestamps(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                normalize_timestamps(item);
            }
        }
        _ => {}
    }
}
