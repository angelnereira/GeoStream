//! CHANGELOG.md format compliance per Keep a Changelog and 19_ENGINEERING_STANDARDS.md §8.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn check_changelog(repo_root: &Path, base_ref: Option<&str>) -> Result<(), Vec<String>> {
    let changelog_path = repo_root.join("CHANGELOG.md");
    let content = match fs::read_to_string(&changelog_path) {
        Ok(c) => c,
        Err(e) => {
            return Err(vec![format!(
                "Cannot read {}: {e}",
                changelog_path.display()
            )]);
        }
    };

    let mut errors = Vec::new();

    if !content.to_lowercase().contains("keepachangelog.com") {
        errors.push("CHANGELOG.md must reference Keep a Changelog standard".to_string());
    }

    if !content.contains("## [Unreleased]") {
        errors.push("CHANGELOG.md must contain a `## [Unreleased]` section".to_string());
    }

    let valid_categories: BTreeSet<&str> = BTreeSet::from([
        "Added",
        "Changed",
        "Deprecated",
        "Removed",
        "Fixed",
        "Security",
        "Docs",
    ]);

    for line in content.lines() {
        if let Some(category_line) = line.strip_prefix("### ") {
            let trimmed = category_line.trim();
            let base_category = trimmed.split('(').next().unwrap_or("").trim();
            if !valid_categories.contains(base_category) {
                errors.push(format!(
                    "Invalid category heading `### {trimmed}`. Must be one of {valid_categories:?}"
                ));
            }
        }
    }

    if let Some(base) = base_ref {
        let diff_range = format!("{base}...HEAD");
        let output = Command::new("git")
            .args(["diff", "--name-only", &diff_range])
            .current_dir(repo_root)
            .output();
        if let Ok(output) = output {
            let diff_files = String::from_utf8_lossy(&output.stdout);
            let changed: Vec<_> = diff_files.lines().map(|s| s.trim()).collect();
            if output.status.success() && !changed.contains(&"CHANGELOG.md") {
                errors.push("CHANGELOG.md has not been updated in this PR diff".to_string());
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!("OK: CHANGELOG.md format complies with Keep a Changelog and 19 §8.");
    Ok(())
}
