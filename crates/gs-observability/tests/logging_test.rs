//! Integration tests for the JSON log formatter of docs/16 §4.3 (T-F2-06).
#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use gs_observability::{
    JsonLogFormatter, LogFields, LogFormat, logging_policy, verify_capture, write_json_line,
};
use serde_json::{Value, json};
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;

type Shared = Arc<Mutex<Vec<u8>>>;

fn capture_subscriber(buffer: Shared) -> impl tracing::Subscriber + Send + Sync {
    let make_writer = move || SharedWriter(buffer.clone());
    tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .json()
            .event_format(JsonLogFormatter)
            .with_writer(make_writer),
    )
}

/// Writer over a shared buffer.
struct SharedWriter(Shared);

impl std::io::Write for SharedWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("lock").extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn captured_lines(buffer: &Shared) -> Vec<Value> {
    let text = String::from_utf8(buffer.lock().expect("lock").clone()).expect("utf8");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
        .collect()
}

#[test]
fn test_json_formatter_writes_the_documented_fields() {
    // covers: NFR-OBS-001, docs/16 §4.3
    let buffer: Shared = Arc::new(Mutex::new(Vec::new()));
    let subscriber = capture_subscriber(buffer.clone());

    tracing::subscriber::with_default(subscriber, || {
        info!(
            request_id = "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a",
            connection_id = "con_0192b6c0a1e47c3f9d2a7b5e8f1c0d3b",
            tenant_id = "ten_0192b6c0a1e47c3f9d2a7b5e8f1c0d3c",
            principal_kind = "device",
            route = "/v1/sessions/{session_id}",
            status = 200u16,
            duration_ms = 12u64,
            error_code = "UNAUTHENTICATED",
            "request accepted"
        );
    });

    let lines = captured_lines(&buffer);
    assert_eq!(lines.len(), 1, "one json line expected: {lines:?}");
    let record = &lines[0];
    assert_eq!(record["msg"], json!("request accepted"));
    assert_eq!(record["level"], json!("INFO"));
    assert!(record.get("target").and_then(Value::as_str).is_some());
    assert!(record.get("ts").and_then(Value::as_str).is_some());
    assert_eq!(
        record["request_id"],
        json!("req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
    );
    assert_eq!(
        record["connection_id"],
        json!("con_0192b6c0a1e47c3f9d2a7b5e8f1c0d3b")
    );
    assert_eq!(
        record["tenant_id"],
        json!("ten_0192b6c0a1e47c3f9d2a7b5e8f1c0d3c")
    );
    assert_eq!(record["principal_kind"], json!("device"));
    assert_eq!(record["route"], json!("/v1/sessions/{session_id}"));
    assert_eq!(record["status"], json!(200));
    assert_eq!(record["duration_ms"], json!(12));
    assert_eq!(record["error_code"], json!("UNAUTHENTICATED"));
}

#[test]
fn test_captured_logs_pass_the_forbidden_pattern_verifier() {
    // covers: FR-PRV-004, NFR-SEC-003, docs/18 §7
    let buffer: Shared = Arc::new(Mutex::new(Vec::new()));
    let subscriber = capture_subscriber(buffer.clone());

    tracing::subscriber::with_default(subscriber, || {
        info!(
            request_id = "req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a",
            tenant_id = "ten_0192b6c0a1e47c3f9d2a7b5e8f1c0d3c",
            "accepted"
        );
        // A redacted secret newtype only ever renders as `<redacted>`.
        let secret = gs_observability::ApiKeySecret::new(
            "gsk_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a.secrettosecrettosecrettosecrettosecret1234",
        );
        info!(secret = ?secret, "key created");
    });

    let text = String::from_utf8(buffer.lock().expect("lock").clone()).expect("utf8");
    let report = verify_capture(&text);
    assert!(
        report.is_clean(),
        "capture must not contain credentials: {:?}",
        report.findings
    );
    assert!(
        text.contains("<redacted>"),
        "the secret must be redacted: {text}"
    );
}

#[test]
fn test_write_json_line_and_policy_agree_with_the_formatter() {
    // covers: NFR-OBS-001, docs/16 §4.3
    let mut buffer: Vec<u8> = Vec::new();
    write_json_line(
        &mut buffer,
        "WARN",
        "geostream::stream",
        "closing",
        &LogFields::new().status(4401u16),
    )
    .expect("write line");
    let report = verify_capture(&String::from_utf8(buffer).expect("utf8"));
    assert!(report.is_clean());

    assert!(matches!(logging_policy("prod", false), Ok(LogFormat::Json)));
    assert!(matches!(
        logging_policy("dev", false),
        Ok(LogFormat::Pretty)
    ));
    assert!(logging_policy("prod", true).is_err());
}
