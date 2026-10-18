//! Log capture verifier (docs/18 §7, docs/10 §8.2).
//!
//! The scenario suite runs with a `tracing` subscriber that keeps everything; this module
//! inspects the captured output and fails when any forbidden value appears: the coordinate
//! values used by the test vectors, `gsk_`, `gst_`, `whsec_`, `eyJ` (JWT) or a generated
//! invitation code. Zero matches (FR-PRV-004, NFR-SEC-003).
#![forbid(unsafe_code)]

/// Forbidden literal patterns of docs/18 §7.
pub const FORBIDDEN_PATTERNS: &[&str] = &["gsk_", "gst_", "whsec_", "eyJ"];

/// Synthetic coordinate values used by the vectors of `contracts/test-vectors/`.
///
/// They are the only coordinates that may appear in a capture: if they do, the redaction
/// of `docs/10` §8.2 has failed.
pub const KNOWN_TEST_COORDINATES: &[&str] = &["8.9824", "-79.5199", "8.982", "-79.519"];

/// Result of verifying a captured log stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureReport {
    /// Lines inspected.
    pub lines: usize,
    /// Forbidden findings with their line number and the offending fragment.
    pub findings: Vec<Finding>,
}

/// One forbidden occurrence in the capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// 1-based line number of the capture.
    pub line: usize,
    /// Forbidden value that was found.
    pub pattern: String,
    /// Bounded excerpt around the occurrence (never the full line).
    pub excerpt: String,
}

impl CaptureReport {
    /// Returns true when nothing forbidden was found.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Decides whether a captured stream is clean.
pub fn verify_capture(capture: &str) -> CaptureReport {
    let mut findings = Vec::new();
    let mut lines = 0usize;

    for (index, line) in capture.lines().enumerate() {
        lines += 1;
        for pattern in KNOWN_TEST_COORDINATES.iter().chain(FORBIDDEN_PATTERNS) {
            if let Some(position) = find_pattern(line, pattern) {
                let start = position.saturating_sub(24);
                let end = (position + pattern.len() + 24).min(line.len());
                findings.push(Finding {
                    line: index + 1,
                    pattern: (*pattern).to_string(),
                    excerpt: line[start..end].to_string(),
                });
            }
        }
    }

    CaptureReport { lines, findings }
}

/// Finds `pattern` in `line` at a position that is not the tail of a longer
/// decimal number.
///
/// The coordinate patterns are prefixes (`"8.982"` matches `"8.9824"`), so a
/// naive `str::find` also matches the fractional part of an ISO-8601 timestamp:
/// `2026-10-18T02:43:18.982593` contains `8.982` right after the `1` of `18`.
/// That is a false positive that made the privacy check fail depending on the
/// clock. A real coordinate is never preceded by a digit, so the match is
/// rejected when the previous character is an ASCII digit.
fn find_pattern(line: &str, pattern: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(relative) = line[from..].find(pattern) {
        let position = from + relative;
        let preceded_by_digit = line[..position]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_digit());
        if !preceded_by_digit {
            return Some(position);
        }
        // Advance past the first byte of the rejected match.
        from = position + 1;
        if from >= line.len() {
            break;
        }
    }
    None
}

/// Asserts that a capture is clean, returning the report otherwise.
pub fn assert_clean_capture(capture: &str) -> Result<CaptureReport, String> {
    let report = verify_capture(capture);
    if report.is_clean() {
        Ok(report)
    } else {
        Err(format!(
            "captured logs contain {} forbidden occurrence(s): {:?}",
            report.findings.len(),
            report.findings
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_capture_passes() {
        // covers: FR-PRV-004, docs/18 §7
        let capture = r#"{"ts":"2026-10-12T12:00:00.000Z","level":"INFO","target":"geostream::api","msg":"accepted","request_id":"req_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a","tenant_id":"ten_0192b6c0a1e47c3f9d2a7b5e8f1c0d3c","status":200,"duration_ms":12}"#;
        let report = verify_capture(capture);
        assert!(
            report.is_clean(),
            "unexpected findings: {:?}",
            report.findings
        );
        assert_eq!(report.lines, 1);
    }

    #[test]
    fn test_synthetic_coordinates_are_detected() {
        // covers: FR-PRV-004, NFR-SEC-003, docs/10 §8.2
        let report =
            verify_capture("{\"msg\":\"position\",\"latitude\":8.9824,\"longitude\":-79.5199}");
        assert!(!report.is_clean());
        assert!(report.findings.iter().any(|f| f.pattern == "8.9824"));
        assert!(report.findings.iter().any(|f| f.pattern == "-79.5199"));
    }

    #[test]
    fn test_timestamp_fraction_is_not_a_coordinate_false_positive() {
        // A timestamp like `...T02:43:18.982593+00:00` contains the substring
        // `8.982`, which is a coordinate prefix. It must NOT be flagged: the
        // match is preceded by the digit `1` of `18`.
        let report = verify_capture(
            "{\"created_at\":\"2026-10-18T02:43:18.982593+00:00\",\"updated_at\":\"2026-10-18T02:43:18.982593+00:00\"}",
        );
        assert!(
            report.is_clean(),
            "a timestamp must not be mistaken for a coordinate: {:?}",
            report.findings
        );
    }

    #[test]
    fn test_coordinate_not_preceded_by_a_digit_is_still_detected() {
        // At the very start of the line (no preceding character) and after a
        // separator, the coordinate is still caught.
        let at_start = verify_capture("8.9824,-79.5199");
        assert!(!at_start.is_clean());
        assert!(at_start.findings.iter().any(|f| f.pattern == "8.9824"));

        let after_colon = verify_capture("{\"latitude\":8.9824}");
        assert!(!after_colon.is_clean());
        assert!(after_colon.findings.iter().any(|f| f.pattern == "8.9824"));
    }

    #[test]
    fn test_credential_and_ticket_patterns_are_detected() {
        // covers: NFR-SEC-003, docs/18 §7
        for forbidden in [
            "gsk_0192b6c0a1e47c3f9d2a7b5e8f1c0d3a",
            "whsec_secret",
            "eyJhbGciOiJFZERTQSJ9",
        ] {
            let report = verify_capture(&format!("{{\"msg\":\"{forbidden}\"}}"));
            assert!(
                report
                    .findings
                    .iter()
                    .any(|f| forbidden.starts_with(f.pattern.as_str())),
                "{forbidden} must be detected"
            );
        }
    }

    #[test]
    fn test_findings_report_line_numbers_without_repeating_the_line() {
        // covers: NFR-SEC-003
        let capture = "first line\nsecond line with gsk_leak\nthird line";
        let report = verify_capture(capture);
        assert_eq!(report.lines, 3);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].line, 2);
        assert!(report.findings[0].excerpt.len() < capture.len());
    }
}
