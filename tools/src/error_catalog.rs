//! ErrorCode catalog parity check across common.proto, OpenAPI, and 09_VALIDATION_ERRORS.md §6.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub fn check_error_catalog(repo_root: &Path) -> Result<(), Vec<String>> {
    let proto_path = repo_root.join("contracts/proto/geostream/v1/common.proto");
    let openapi_path = repo_root.join("contracts/openapi/geostream.v1.yaml");
    let doc_path = repo_root.join("docs/09_VALIDATION_ERRORS.md");

    // 1. Parse common.proto
    let proto_content = match fs::read_to_string(&proto_path) {
        Ok(c) => c,
        Err(e) => return Err(vec![format!("Cannot read {}: {e}", proto_path.display())]),
    };

    let mut proto_codes = BTreeSet::new();
    let mut in_enum = false;
    for line in proto_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("enum ErrorCode") {
            in_enum = true;
            continue;
        }
        if in_enum {
            if trimmed.starts_with('}') {
                break;
            }
            if let Some((name, _)) = trimmed
                .strip_prefix("ERROR_CODE_")
                .and_then(|r| r.split_once('='))
            {
                let code = name.trim();
                if code != "UNSPECIFIED" {
                    proto_codes.insert(code.to_string());
                }
            }
        }
    }

    // 2. Parse OpenAPI ErrorCode
    let openapi_content = match fs::read_to_string(&openapi_path) {
        Ok(c) => c,
        Err(e) => return Err(vec![format!("Cannot read {}: {e}", openapi_path.display())]),
    };

    let mut openapi_codes = BTreeSet::new();
    let mut in_error_code = false;
    let mut in_enum_block = false;
    for line in openapi_content.lines() {
        if line.trim().starts_with("ErrorCode:") {
            in_error_code = true;
            continue;
        }
        if in_error_code {
            let trimmed = line.trim();
            if trimmed == "enum:" {
                in_enum_block = true;
                continue;
            }
            if in_enum_block {
                if let Some(code) = trimmed.strip_prefix("- ") {
                    openapi_codes.insert(code.trim().to_string());
                } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    break;
                }
            }
        }
    }

    // 3. Parse docs/09_VALIDATION_ERRORS.md §6
    let doc_content = match fs::read_to_string(&doc_path) {
        Ok(c) => c,
        Err(e) => return Err(vec![format!("Cannot read {}: {e}", doc_path.display())]),
    };

    let mut doc_codes = BTreeSet::new();
    let mut in_sec6 = false;
    for line in doc_content.lines() {
        if line.contains("## 6. Catálogo de errores") {
            in_sec6 = true;
            continue;
        }
        if in_sec6 {
            if line.starts_with("## 7.") {
                break;
            }
            let trimmed = line.trim();
            if trimmed.starts_with('|') {
                let cols: Vec<_> = trimmed.split('|').map(|s| s.trim()).collect();
                if cols.len() >= 3 && cols[1].starts_with('`') && cols[1].ends_with('`') {
                    let code = cols[1].trim_matches('`');
                    if code.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                        doc_codes.insert(code.to_string());
                    }
                }
            }
        }
    }

    let mut errors = Vec::new();
    if proto_codes != openapi_codes {
        let in_proto_only: Vec<_> = proto_codes.difference(&openapi_codes).collect();
        let in_openapi_only: Vec<_> = openapi_codes.difference(&proto_codes).collect();
        if !in_proto_only.is_empty() {
            errors.push(format!(
                "In common.proto but not in OpenAPI: {in_proto_only:?}"
            ));
        }
        if !in_openapi_only.is_empty() {
            errors.push(format!(
                "In OpenAPI but not in common.proto: {in_openapi_only:?}"
            ));
        }
    }

    if proto_codes != doc_codes {
        let in_proto_only: Vec<_> = proto_codes.difference(&doc_codes).collect();
        let in_doc_only: Vec<_> = doc_codes.difference(&proto_codes).collect();
        if !in_proto_only.is_empty() {
            errors.push(format!(
                "In common.proto but not in 09_VALIDATION_ERRORS.md: {in_proto_only:?}"
            ));
        }
        if !in_doc_only.is_empty() {
            errors.push(format!(
                "In 09_VALIDATION_ERRORS.md but not in common.proto: {in_doc_only:?}"
            ));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!(
        "OK: All {} error codes match across common.proto, OpenAPI, and 09_VALIDATION_ERRORS.md.",
        proto_codes.len()
    );
    Ok(())
}
