//! Direct binary entrypoint for check_crate_deps per 04_ARCHITECTURE.md §4.1.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(manifest);
        if p.file_name().and_then(|s| s.to_str()) == Some("tools") {
            p.parent().unwrap_or(&p).to_path_buf()
        } else {
            p
        }
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };

    match geostream_tools::crate_deps::check_crate_deps(&root) {
        Ok(()) => ExitCode::SUCCESS,
        Err(errors) => {
            eprintln!("FAIL: Crate dependency check failed:");
            for err in errors {
                eprintln!("  - {err}");
            }
            ExitCode::FAILURE
        }
    }
}
