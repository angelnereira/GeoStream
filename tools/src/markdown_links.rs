//! Relative markdown links validity check across the repository.
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

fn collect_all_md_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !name.starts_with('.') && name != "target" && name != "node_modules" {
                collect_all_md_files(&path, files)?;
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
}

fn strip_code_blocks(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_code_block = false;
    for line in content.lines() {
        if line.trim().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if !in_code_block {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

pub fn check_markdown_links(repo_root: &Path) -> Result<(), Vec<String>> {
    let mut md_files = Vec::new();
    if let Err(e) = collect_all_md_files(repo_root, &mut md_files) {
        return Err(vec![format!("Cannot scan repository files: {e}")]);
    }

    let mut errors = Vec::new();
    let mut total_links = 0;

    for path in &md_files {
        let rel_path = path.strip_prefix(repo_root).unwrap_or(path);
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{}: read error: {e}", rel_path.display()));
                continue;
            }
        };

        let parent = path.parent().unwrap_or(repo_root);
        let stripped = strip_code_blocks(&content);

        // Find [text](target)
        let mut idx = 0;
        while let Some(open_bracket) = stripped[idx..].find('[') {
            let text_start = idx + open_bracket;
            if let Some(close_bracket) = stripped[text_start..].find(']') {
                let text_end = text_start + close_bracket;
                if text_end + 1 < stripped.len() && stripped.as_bytes()[text_end + 1] == b'(' {
                    let link_start = text_end + 2;
                    if let Some(close_paren) = stripped[link_start..].find(')') {
                        let link_end = link_start + close_paren;
                        let link = stripped[link_start..link_end].trim();
                        idx = link_end + 1;

                        if link.is_empty()
                            || link.starts_with('#')
                            || link.starts_with("http://")
                            || link.starts_with("https://")
                            || link.starts_with("mailto:")
                            || link.contains("://")
                        {
                            continue;
                        }

                        total_links += 1;
                        let target_file = link.split('#').next().unwrap_or("").trim();
                        if target_file.is_empty() {
                            continue;
                        }

                        let resolved = parent.join(target_file);
                        if !resolved.exists() {
                            errors.push(format!(
                                "In {}: broken relative link `{link}` (resolved: {})",
                                rel_path.display(),
                                resolved.display()
                            ));
                        }
                        continue;
                    }
                }
            }
            idx = text_start + 1;
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!("OK: All {total_links} relative markdown links successfully verified.");
    Ok(())
}
