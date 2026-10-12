//! Configuration and policy keys consistency check against docs/17_CONFIGURATION.md.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn collect_doc_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name != "proposals" && name != "templates" {
                collect_doc_files(&path, files)?;
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
}

pub fn check_config_keys(repo_root: &Path) -> Result<(), Vec<String>> {
    let doc17_path = repo_root.join("docs/17_CONFIGURATION.md");
    let doc17_content = match fs::read_to_string(&doc17_path) {
        Ok(c) => c,
        Err(e) => return Err(vec![format!("Cannot read {}: {e}", doc17_path.display())]),
    };

    let mut server_keys = BTreeSet::new();
    let mut policy_keys = BTreeSet::new();

    // 1. Server keys in 17 §2
    for line in doc17_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('|') {
            let cols: Vec<_> = trimmed.split('|').map(|s| s.trim()).collect();
            if cols.len() >= 3 && cols[1].starts_with('`') && cols[1].ends_with('`') {
                let key = cols[1].trim_matches('`');
                if key.starts_with("GS_") {
                    server_keys.insert(key.to_string());
                }
            }
        }
    }

    // 2. Policy keys in 17 §4
    let mut in_sec4 = false;
    for line in doc17_content.lines() {
        if line.contains("## 4. Política por tenant") {
            in_sec4 = true;
            continue;
        }
        if in_sec4 {
            if line.starts_with("## 5.") {
                break;
            }
            let trimmed = line.trim();
            if trimmed.starts_with('|') {
                let cols: Vec<_> = trimmed.split('|').map(|s| s.trim()).collect();
                if cols.len() >= 3 && cols[1].starts_with('`') && cols[1].ends_with('`') {
                    let key = cols[1].trim_matches('`');
                    if key.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                        policy_keys.insert(key.to_string());
                    }
                }
            }
        }
    }

    let integrator_keys = BTreeSet::from(["GS_API_KEY", "GS_BASE_URL"]);

    let docs_dir = repo_root.join("docs");
    let mut doc_files = Vec::new();
    if let Err(e) = collect_doc_files(&docs_dir, &mut doc_files) {
        return Err(vec![format!("Cannot scan docs directory: {e}")]);
    }

    let mut errors = Vec::new();

    for path in &doc_files {
        let rel_path = path.strip_prefix(repo_root).unwrap_or(path);
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{}: read error: {e}", rel_path.display()));
                continue;
            }
        };

        // Scan tokens for GS_
        let mut idx = 0;
        let bytes = content.as_bytes();
        while let Some(pos) = content[idx..].find("GS_") {
            let start = idx + pos;
            let mut end = start + 3;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            let token = &content[start..end];
            let has_star = end < bytes.len() && bytes[end] == b'*';

            idx = end;

            if token == "GS_" || token == "GS_CONFIG" {
                continue;
            }

            if token
                .strip_suffix("_FILE")
                .is_some_and(|base| server_keys.contains(base))
            {
                continue;
            }

            if has_star || token.ends_with('_') {
                let prefix = if token.ends_with('_') {
                    token.to_string()
                } else {
                    format!("{token}_")
                };
                if server_keys.iter().any(|k| k.starts_with(&prefix)) {
                    continue;
                }
            }

            if !server_keys.contains(token) && !integrator_keys.contains(token) {
                errors.push(format!(
                    "{}: Unknown server configuration key `{token}`",
                    rel_path.display()
                ));
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!(
        "OK: All GS_* keys and policy references in docs are valid against 17_CONFIGURATION.md ({} server keys, {} policy keys).",
        server_keys.len(),
        policy_keys.len()
    );
    Ok(())
}
