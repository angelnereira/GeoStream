//! Requirement IDs and Traceability parity check across docs and TRACEABILITY.md.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn is_req_id(token: &str) -> bool {
    let rest = if let Some(r) = token.strip_prefix("FR-") {
        r
    } else if let Some(r) = token.strip_prefix("NFR-") {
        r
    } else {
        return false;
    };

    let parts: Vec<&str> = rest.split('-').collect();
    if parts.len() != 2 {
        return false;
    }

    let cat = parts[0];
    let num = parts[1];

    !cat.is_empty()
        && cat.chars().all(|c| c.is_ascii_uppercase())
        && num.len() == 3
        && num.chars().all(|c| c.is_ascii_digit())
}

fn extract_req_ids(text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for word in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
        if is_req_id(word) {
            ids.insert(word.to_string());
        }
    }
    ids
}

fn collect_doc_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name != "proposals" {
                collect_doc_files(&path, files)?;
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
}

pub fn check_requirements(repo_root: &Path) -> Result<(), Vec<String>> {
    let req_path = repo_root.join("docs/03_REQUIREMENTS.md");
    let trace_path = repo_root.join("docs/TRACEABILITY.md");

    let req_content = match fs::read_to_string(&req_path) {
        Ok(c) => c,
        Err(e) => return Err(vec![format!("Cannot read {}: {e}", req_path.display())]),
    };
    let defined_reqs = extract_req_ids(&req_content);

    let trace_content = match fs::read_to_string(&trace_path) {
        Ok(c) => c,
        Err(e) => return Err(vec![format!("Cannot read {}: {e}", trace_path.display())]),
    };
    let traced_reqs = extract_req_ids(&trace_content);

    let mut errors = Vec::new();

    let in_03_only: Vec<_> = defined_reqs.difference(&traced_reqs).collect();
    let in_trace_only: Vec<_> = traced_reqs.difference(&defined_reqs).collect();
    if !in_03_only.is_empty() {
        errors.push(format!(
            "In 03_REQUIREMENTS.md but not in TRACEABILITY.md: {in_03_only:?}"
        ));
    }
    if !in_trace_only.is_empty() {
        errors.push(format!(
            "In TRACEABILITY.md but not in 03_REQUIREMENTS.md: {in_trace_only:?}"
        ));
    }

    let docs_dir = repo_root.join("docs");
    let mut doc_files = Vec::new();
    if let Err(e) = collect_doc_files(&docs_dir, &mut doc_files) {
        return Err(vec![format!("Cannot scan docs directory: {e}")]);
    }

    for path in &doc_files {
        let rel_path = path.strip_prefix(repo_root).unwrap_or(path);
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{}: read error: {e}", rel_path.display()));
                continue;
            }
        };

        let found = extract_req_ids(&content);
        let unknown: Vec<_> = found.difference(&defined_reqs).collect();
        if !unknown.is_empty() {
            errors.push(format!(
                "{}: references unknown requirement IDs: {unknown:?}",
                rel_path.display()
            ));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!(
        "OK: All {} requirement IDs verified across 03_REQUIREMENTS.md, TRACEABILITY.md, and all normative docs.",
        defined_reqs.len()
    );
    Ok(())
}
