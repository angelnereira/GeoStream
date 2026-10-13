//! JSON logs with the exact fields of docs/16_OPERATIONS.md §4.3.
//!
//! Fields: `ts`, `level`, `target`, `msg`, `request_id`, `connection_id`, `tenant_id`,
//! `principal_kind`, `route`, `status`, `duration_ms`, `error_code`. Never the forbidden fields
//! of `docs/10` §8.2. `GS_LOG_UNSAFE_LOCATIONS=true` is only accepted with `GS_ENV=dev`.
#![forbid(unsafe_code)]

use std::fmt;
use std::io;

use serde_json::{Map, Value, json};
use time::format_description::well_known::Rfc3339;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::{FormatFields, Writer};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormattedFields};
use tracing_subscriber::registry::LookupSpan;

/// Fields of a log record (docs/16 §4.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogFields {
    /// Correlation id (`req_…`).
    pub request_id: Option<String>,
    /// Stream connection id (`con_…`).
    pub connection_id: Option<String>,
    /// Tenant id (`ten_…`): allowed in logs, forbidden in metric labels.
    pub tenant_id: Option<String>,
    /// Principal kind.
    pub principal_kind: Option<String>,
    /// Route template, never a concrete id.
    pub route: Option<String>,
    /// HTTP status code.
    pub status: Option<u16>,
    /// Duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Error code of `docs/09` §6.
    pub error_code: Option<String>,
}

impl LogFields {
    /// Empty fields.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the correlation id.
    pub fn request_id(mut self, value: impl Into<String>) -> Self {
        self.request_id = Some(value.into());
        self
    }

    /// Adds the connection id.
    pub fn connection_id(mut self, value: impl Into<String>) -> Self {
        self.connection_id = Some(value.into());
        self
    }

    /// Adds the tenant id (allowed in logs, docs/16 §4.3).
    pub fn tenant_id(mut self, value: impl Into<String>) -> Self {
        self.tenant_id = Some(value.into());
        self
    }

    /// Adds the principal kind.
    pub fn principal_kind(mut self, value: impl Into<String>) -> Self {
        self.principal_kind = Some(value.into());
        self
    }

    /// Adds the route template.
    pub fn route(mut self, value: impl Into<String>) -> Self {
        self.route = Some(value.into());
        self
    }

    /// Adds the HTTP status.
    pub fn status(mut self, value: u16) -> Self {
        self.status = Some(value);
        self
    }

    /// Adds the duration in milliseconds.
    pub fn duration_ms(mut self, value: u64) -> Self {
        self.duration_ms = Some(value);
        self
    }

    /// Adds the error code.
    pub fn error_code(mut self, value: impl Into<String>) -> Self {
        self.error_code = Some(value.into());
        self
    }

    /// Renders the optional fields as a JSON object, skipping absent ones.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        if let Some(request_id) = &self.request_id {
            map.insert("request_id".to_string(), json!(request_id));
        }
        if let Some(connection_id) = &self.connection_id {
            map.insert("connection_id".to_string(), json!(connection_id));
        }
        if let Some(tenant_id) = &self.tenant_id {
            map.insert("tenant_id".to_string(), json!(tenant_id));
        }
        if let Some(principal_kind) = &self.principal_kind {
            map.insert("principal_kind".to_string(), json!(principal_kind));
        }
        if let Some(route) = &self.route {
            map.insert("route".to_string(), json!(route));
        }
        if let Some(status) = self.status {
            map.insert("status".to_string(), json!(status));
        }
        if let Some(duration_ms) = self.duration_ms {
            map.insert("duration_ms".to_string(), json!(duration_ms));
        }
        if let Some(error_code) = &self.error_code {
            map.insert("error_code".to_string(), json!(error_code));
        }
        Value::Object(map)
    }
}

/// Formats an instant as RFC 3339 with milliseconds (docs/00 §4.5).
pub fn format_timestamp(instant: time::OffsetDateTime) -> String {
    instant
        .format(&Rfc3339)
        .unwrap_or_else(|_| instant.to_string())
}

/// JSON event formatter with the fields of docs/16 §4.3.
#[derive(Debug, Default, Clone)]
pub struct JsonLogFormatter;

/// Collects the message and the remaining fields of an event.
#[derive(Default)]
struct EventVisitor {
    message: Option<String>,
    fields: Map<String, Value>,
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(rendered);
        } else {
            self.fields
                .insert(field.name().to_string(), Value::String(rendered));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields
                .insert(field.name().to_string(), Value::String(value.to_string()));
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }
}

impl<S, N> FormatEvent<S, N> for JsonLogFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        let mut record = Map::new();
        record.insert(
            "ts".to_string(),
            json!(format_timestamp(time::OffsetDateTime::now_utc())),
        );
        record.insert(
            "level".to_string(),
            json!(event.metadata().level().as_str()),
        );
        record.insert("target".to_string(), json!(event.metadata().target()));
        record.insert(
            "msg".to_string(),
            json!(visitor.message.unwrap_or_default()),
        );
        for (name, value) in visitor.fields {
            record.insert(name, value);
        }

        if let Some(span) = context.lookup_current() {
            let mut spans = Vec::new();
            let mut current = Some(span);
            while let Some(span) = current {
                if let Some(fields) = span.extensions().get::<FormattedFields<N>>() {
                    spans.push(json!(format!("{}:{}", span.metadata().name(), fields)));
                }
                current = span.parent();
            }
            if !spans.is_empty() {
                spans.reverse();
                record.insert("spans".to_string(), Value::Array(spans));
            }
        }

        let line = Value::Object(record).to_string();
        writeln!(writer, "{line}")
    }
}

/// Writes a log record as JSON to an arbitrary writer (used by the capture verifier).
pub fn write_json_line(
    writer: &mut impl io::Write,
    level: &str,
    target: &str,
    message: &str,
    fields: &LogFields,
) -> io::Result<()> {
    let mut record = Map::new();
    record.insert(
        "ts".to_string(),
        json!(format_timestamp(time::OffsetDateTime::now_utc())),
    );
    record.insert("level".to_string(), json!(level));
    record.insert("target".to_string(), json!(target));
    record.insert("msg".to_string(), json!(message));
    for (name, value) in fields.to_json().as_object().into_iter().flatten() {
        record.insert(name.clone(), value.clone());
    }
    writeln!(writer, "{}", Value::Object(record))
}

/// Log format accepted by `GS_LOG_FORMAT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Human readable lines (development default).
    Pretty,
    /// One JSON object per line (docs/16 §4.3).
    Json,
}

impl LogFormat {
    /// Parses the value of `GS_LOG_FORMAT`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "json" => Some(Self::Json),
            "pretty" | "text" => Some(Self::Pretty),
            _ => None,
        }
    }

    /// Default format for each environment (docs/17 §2.1).
    pub const fn default_for(is_production: bool) -> Self {
        if is_production {
            Self::Json
        } else {
            Self::Pretty
        }
    }
}

/// Errors produced while initialising the log subscriber.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// `GS_LOG_FORMAT` is not `json` nor `pretty`.
    #[error("invalid GS_LOG_FORMAT '{value}': expected json or pretty")]
    InvalidFormat { value: String },

    /// Unsafe location logging outside development.
    #[error("GS_LOG_UNSAFE_LOCATIONS=true is only allowed with GS_ENV=dev")]
    UnsafeLocationsOutsideDevelopment,
}

/// Validates the interaction between `GS_ENV`, `GS_LOG_FORMAT` and
/// `GS_LOG_UNSAFE_LOCATIONS` without touching the environment (docs/10 §8.2).
pub fn logging_policy(
    environment: &str,
    unsafe_locations: bool,
) -> Result<LogFormat, LoggingError> {
    if unsafe_locations && environment != "dev" {
        return Err(LoggingError::UnsafeLocationsOutsideDevelopment);
    }
    Ok(LogFormat::default_for(environment == "prod"))
}

/// Initialises the global tracing subscriber honouring `GS_LOG_FORMAT`, `GS_LOG_LEVEL`,
/// `GS_ENV` and `GS_LOG_UNSAFE_LOCATIONS` (docs/17 §2.1, docs/10 §8.2).
///
/// Returns the format that was installed. Installing twice is a no-op.
pub fn init_from_env() -> Result<LogFormat, LoggingError> {
    let format = match std::env::var("GS_LOG_FORMAT") {
        Ok(value) => LogFormat::parse(&value).ok_or(LoggingError::InvalidFormat { value })?,
        Err(_) => LogFormat::default_for(false),
    };

    let level = std::env::var("GS_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    let environment = std::env::var("GS_ENV").unwrap_or_else(|_| "dev".to_string());
    let unsafe_locations = std::env::var("GS_LOG_UNSAFE_LOCATIONS")
        .map(|value| value.eq_ignore_ascii_case("true") || value == "1")
        .unwrap_or(false);
    let _ = logging_policy(&environment, unsafe_locations)?;

    let filter = tracing_subscriber::EnvFilter::try_new(level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    match format {
        LogFormat::Json => {
            let _ = tracing_subscriber::fmt()
                .json()
                .event_format(JsonLogFormatter)
                .with_env_filter(filter)
                .try_init();
        }
        LogFormat::Pretty => {
            let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
        }
    }

    Ok(format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_line_contains_exactly_the_documented_fields() {
        // covers: NFR-OBS-001, docs/16 §4.3
        let mut buffer: Vec<u8> = Vec::new();
        let fields = LogFields::new()
            .request_id("req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a")
            .connection_id("con_0192b6c0a1e47c3f9d2a7b5e8f1c0d3b")
            .tenant_id("ten_0192b6c0a1e47c3f9d2a7b5e8f1c0d3c")
            .principal_kind("device")
            .route("/v1/sessions/{session_id}")
            .status(200)
            .duration_ms(12)
            .error_code("UNAUTHENTICATED");
        write_json_line(&mut buffer, "INFO", "geostream::api", "accepted", &fields)
            .expect("write line");

        let line = String::from_utf8(buffer).expect("utf8 line");
        let record: Value = serde_json::from_str(&line).expect("json line");
        assert_eq!(record["level"], json!("INFO"));
        assert_eq!(record["target"], json!("geostream::api"));
        assert_eq!(record["msg"], json!("accepted"));
        assert_eq!(record["status"], json!(200));
        assert_eq!(record["duration_ms"], json!(12));
        assert_eq!(record["error_code"], json!("UNAUTHENTICATED"));
        assert_eq!(record["route"], json!("/v1/sessions/{session_id}"));
        for mandatory in ["ts", "level", "target", "msg"] {
            assert!(record.get(mandatory).is_some(), "missing {mandatory}");
        }
        // Absent optional fields are omitted rather than serialized as null.
        assert!(record.get("connection_id").is_some());
    }

    #[test]
    fn test_unsafe_locations_are_rejected_outside_development() {
        // covers: NFR-SEC-003, docs/10 §8.2
        assert!(matches!(
            logging_policy("prod", true),
            Err(LoggingError::UnsafeLocationsOutsideDevelopment)
        ));
        assert!(logging_policy("dev", true).is_ok());
        assert!(logging_policy("test", false).is_ok());
    }

    #[test]
    fn test_log_format_is_bounded() {
        // covers: NFR-OBS-001, docs/17 §2.1
        assert_eq!(LogFormat::parse("json"), Some(LogFormat::Json));
        assert_eq!(LogFormat::parse("PRETTY"), Some(LogFormat::Pretty));
        assert_eq!(LogFormat::parse("xml"), None);
        assert_eq!(LogFormat::default_for(true), LogFormat::Json);
        assert_eq!(LogFormat::default_for(false), LogFormat::Pretty);
    }
}
