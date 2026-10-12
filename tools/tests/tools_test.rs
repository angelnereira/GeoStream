//! Tests for geostream-tools consistency verification.
#![forbid(unsafe_code)]

use std::path::PathBuf;

fn get_repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(&manifest).to_path_buf()
}

#[test]
fn test_contracts_json_validity() {
    let root = get_repo_root();
    let res = std::process::Command::new(env!("CARGO_BIN_EXE_geostream-tools"))
        .args(["--repo-root", root.to_str().unwrap(), "json"])
        .output()
        .expect("failed to execute geostream-tools");
    assert!(
        res.status.success(),
        "JSON validity check failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
}

#[test]
fn test_all_consistency_checks() {
    let root = get_repo_root();
    let res = std::process::Command::new(env!("CARGO_BIN_EXE_geostream-tools"))
        .args(["--repo-root", root.to_str().unwrap(), "all"])
        .output()
        .expect("failed to execute geostream-tools");
    assert!(
        res.status.success(),
        "All consistency checks failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
}
