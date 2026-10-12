//! JSON validity check for contracts/
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

fn collect_json_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
            files.push(path);
        }
    }
    Ok(())
}

pub fn check_json(repo_root: &Path) -> Result<(), Vec<String>> {
    let contracts_dir = repo_root.join("contracts");
    let mut files = Vec::new();
    if let Err(e) = collect_json_files(&contracts_dir, &mut files) {
        return Err(vec![format!("Failed to read contracts dir: {e}")]);
    }

    let mut errors = Vec::new();
    let mut count = 0;

    for path in &files {
        let rel_path = path.strip_prefix(repo_root).unwrap_or(path);
        match fs::read_to_string(path) {
            Ok(content) => {
                if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
                    errors.push(format!("{}: {}", rel_path.display(), e));
                } else {
                    count += 1;
                }
            }
            Err(e) => {
                errors.push(format!("{}: read error: {}", rel_path.display(), e));
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!("OK: All {count} JSON files under contracts/ are valid.");
    Ok(())
}
