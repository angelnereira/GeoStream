//! Auth matrix parity check between contracts/openapi/geostream.v1.yaml and docs/06_IDENTITY_AUTH.md §8.2.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

struct AuthScope {
    app: String,
    dev: String,
}

fn normalize_doc_scope(val: &str) -> String {
    let val = val.trim();
    if val == "✗" {
        return "forbidden".to_string();
    }
    if val == "—" || val == "sin autenticación" {
        return "none".to_string();
    }
    if let Some(code) = val
        .split_once('`')
        .and_then(|(_, r)| r.split_once('`'))
        .map(|(c, _)| c)
    {
        return code.trim().to_string();
    }
    val.to_string()
}

pub fn check_auth_matrix(repo_root: &Path) -> Result<(), Vec<String>> {
    let doc_path = repo_root.join("docs/06_IDENTITY_AUTH.md");
    let openapi_path = repo_root.join("contracts/openapi/geostream.v1.yaml");

    let doc_content = match fs::read_to_string(&doc_path) {
        Ok(c) => c,
        Err(e) => return Err(vec![format!("Cannot read {}: {e}", doc_path.display())]),
    };

    let mut doc_ops = BTreeMap::new();
    let mut in_sec82 = false;
    for line in doc_content.lines() {
        if line.contains("### 8.2 Matriz REST") {
            in_sec82 = true;
            continue;
        }
        if in_sec82 {
            if line.starts_with("### 8.3") {
                break;
            }
            let trimmed = line.trim();
            if trimmed.starts_with('|')
                && !trimmed.contains("operationId")
                && !trimmed.contains("---")
            {
                let cols: Vec<_> = trimmed.split('|').map(|s| s.trim()).collect();
                if cols.len() >= 4 {
                    let op_col = cols[1];
                    let app_col = cols[2];
                    let dev_col = cols[3];

                    // Extract all `op` in op_col
                    let mut start_idx = 0;
                    while let Some(pos) = op_col[start_idx..].find('`') {
                        let actual_start = start_idx + pos + 1;
                        if let Some(end_pos) = op_col[actual_start..].find('`') {
                            let op_name = &op_col[actual_start..actual_start + end_pos];
                            doc_ops.insert(
                                op_name.to_string(),
                                AuthScope {
                                    app: normalize_doc_scope(app_col),
                                    dev: normalize_doc_scope(dev_col),
                                },
                            );
                            start_idx = actual_start + end_pos + 1;
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }

    let openapi_content = match fs::read_to_string(&openapi_path) {
        Ok(c) => c,
        Err(e) => return Err(vec![format!("Cannot read {}: {e}", openapi_path.display())]),
    };

    let mut openapi_ops: BTreeMap<String, AuthScope> = BTreeMap::new();
    let mut curr_op: Option<String> = None;

    for line in openapi_content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("operationId:") {
            let op = rest.trim();
            curr_op = Some(op.to_string());
        }
        if let (Some(op), Some(auth_pos)) = (&curr_op, line.find("x-gs-auth:")) {
            let auth_str = line[auth_pos + "x-gs-auth:".len()..].trim();
            if auth_str.starts_with('{') && auth_str.ends_with('}') {
                let inner = &auth_str[1..auth_str.len() - 1];
                let mut app_val = String::new();
                let mut dev_val = String::new();
                for part in inner.split(',') {
                    let part = part.trim();
                    if let Some((k, v)) = part.split_once(':') {
                        let k = k.trim();
                        let v = v.trim().trim_matches('\'').trim_matches('"');
                        if k == "application" {
                            app_val = v.to_string();
                        } else if k == "device" {
                            dev_val = v.to_string();
                        }
                    }
                }
                openapi_ops.insert(
                    op.clone(),
                    AuthScope {
                        app: app_val,
                        dev: dev_val,
                    },
                );
            }
        }
    }

    let mut errors = Vec::new();

    for (op, doc_scope) in &doc_ops {
        if let Some(o_scope) = openapi_ops.get(op) {
            if doc_scope.app != o_scope.app {
                errors.push(format!(
                    "Operation `{op}` application scope mismatch: doc='{}', openapi='{}'",
                    doc_scope.app, o_scope.app
                ));
            }
            if doc_scope.dev != o_scope.dev {
                errors.push(format!(
                    "Operation `{op}` device scope mismatch: doc='{}', openapi='{}'",
                    doc_scope.dev, o_scope.dev
                ));
            }
        } else {
            errors.push(format!(
                "Operation `{op}` in 06_IDENTITY_AUTH.md §8.2 does not exist in OpenAPI"
            ));
        }
    }

    for op in openapi_ops.keys() {
        if (op.starts_with("on") && op.ends_with("Event")) || op == "openStream" {
            continue;
        }
        if !doc_ops.contains_key(op) {
            errors.push(format!(
                "OpenAPI operation `{op}` is not documented in 06_IDENTITY_AUTH.md §8.2"
            ));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!(
        "OK: All {} REST operations and auth scopes match perfectly with OpenAPI.",
        doc_ops.len()
    );
    Ok(())
}
