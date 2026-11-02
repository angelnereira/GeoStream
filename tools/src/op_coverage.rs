//! Every operation of the OpenAPI is named by at least one test (`docs/18` §5), as a ratchet.
//!
//! Why: `POST /v1/ingest/locations` shipped broken against its own contract (enum names and a zero
//! event age) because no HTTP test named it, and nothing noticed. The check looks for the
//! `operationId` as a string literal (`"ingestLocations"`) in the test sources, which is how the
//! contract-validating suites pass it to `check_request` / `check_response`.
//!
//! It is a *ratchet*: the operations that had no test when the check was introduced are listed in
//! `tools/op_coverage_baseline.txt`. A new operation without a test fails the check; an operation of
//! the baseline that gets a test must leave the list, so the list only ever shrinks.
//!
//! It is a heuristic by design: a test that names the operation but asserts nothing passes it. The
//! point is to make the absence of any test visible, not to prove the quality of the ones that exist.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const OPENAPI: &str = "contracts/openapi/geostream.v1.yaml";
const BASELINE: &str = "tools/op_coverage_baseline.txt";

/// `operationId`s of the HTTP operations (the `paths:` section; the `webhooks:` callbacks are
/// outbound events, not operations a test can call).
pub fn path_operations(openapi: &str) -> BTreeSet<String> {
    let mut section = "";
    let mut ops = BTreeSet::new();
    for line in openapi.lines() {
        if !line.starts_with(' ') && line.ends_with(':') && !line.starts_with('#') {
            section = line.trim_end_matches(':');
        }
        if section == "paths"
            && let Some(rest) = line.trim_start().strip_prefix("operationId:")
        {
            ops.insert(rest.trim().to_string());
        }
    }
    ops
}

/// Operations named in a baseline file (`#` starts a comment).
pub fn parse_baseline(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn collect_rs(dir: &Path, out: &mut String) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(text) = fs::read_to_string(&path)
        {
            out.push_str(&text);
            out.push('\n');
        }
    }
}

/// All the Rust test sources: `crates/*/tests` and `apps/*/tests`.
fn test_sources(repo_root: &Path) -> String {
    let mut text = String::new();
    for group in ["crates", "apps"] {
        let Ok(entries) = fs::read_dir(repo_root.join(group)) else {
            continue;
        };
        for entry in entries.flatten() {
            let tests: PathBuf = entry.path().join("tests");
            collect_rs(&tests, &mut text);
        }
    }
    text
}

/// Pure core of the check, over already-read inputs.
pub fn evaluate(
    operations: &BTreeSet<String>,
    tests: &str,
    baseline: &BTreeSet<String>,
) -> Vec<String> {
    let covered: BTreeSet<&String> = operations
        .iter()
        .filter(|op| tests.contains(&format!("\"{op}\"")))
        .collect();
    let mut errors = Vec::new();
    for op in operations {
        let is_covered = covered.contains(op);
        let in_baseline = baseline.contains(op);
        if !is_covered && !in_baseline {
            errors.push(format!(
                "operation `{op}` is named by no test: add an HTTP test that validates it against the OpenAPI (do not add it to {BASELINE})"
            ));
        }
        if is_covered && in_baseline {
            errors.push(format!(
                "operation `{op}` now has a test: remove it from {BASELINE} so the gap list only shrinks"
            ));
        }
    }
    for op in baseline {
        if !operations.contains(op) {
            errors.push(format!(
                "{BASELINE} lists `{op}`, which is not an operation of the OpenAPI: remove it"
            ));
        }
    }
    errors
}

pub fn check_operation_coverage(repo_root: &Path) -> Result<(), Vec<String>> {
    let read = |rel: &str| {
        fs::read_to_string(repo_root.join(rel)).map_err(|e| vec![format!("Cannot read {rel}: {e}")])
    };
    let operations = path_operations(&read(OPENAPI)?);
    let baseline = parse_baseline(&read(BASELINE)?);
    let errors = evaluate(&operations, &test_sources(repo_root), &baseline);
    if errors.is_empty() {
        let uncovered = baseline.len();
        println!(
            "{} operations; {} named by a test, {} still on the gap list ({BASELINE}).",
            operations.len(),
            operations.len() - uncovered,
            uncovered
        );
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = "openapi: 3.1.0\npaths:\n  /a:\n    get:\n      operationId: getA\n  /b:\n    post:\n      operationId: postB\nwebhooks:\n  ev:\n    post:\n      operationId: onEv\ncomponents:\n  x: 1\n";

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn test_only_path_operations_are_collected() {
        assert_eq!(path_operations(YAML), set(&["getA", "postB"]));
    }

    #[test]
    fn test_a_new_operation_without_a_test_fails() {
        let errors = evaluate(&set(&["getA", "postB"]), "x(\"getA\")", &set(&[]));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("postB"));
    }

    #[test]
    fn test_a_baseline_operation_that_gets_covered_must_leave_the_list() {
        let errors = evaluate(&set(&["getA"]), "x(\"getA\")", &set(&["getA"]));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("remove it"));
    }

    #[test]
    fn test_a_stale_baseline_entry_fails() {
        let errors = evaluate(&set(&["getA"]), "x(\"getA\")", &set(&["gone"]));
        assert!(errors.iter().any(|e| e.contains("gone")));
    }

    #[test]
    fn test_covered_and_baselined_operations_pass() {
        let errors = evaluate(&set(&["getA", "postB"]), "x(\"getA\")", &set(&["postB"]));
        assert!(errors.is_empty());
    }

    #[test]
    fn test_baseline_comments_are_ignored() {
        assert_eq!(
            parse_baseline("# header\nfoo # why\n\nbar\n"),
            set(&["foo", "bar"])
        );
    }
}
