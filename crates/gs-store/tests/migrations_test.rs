//! Embedded-migration guards (no database required).
//!
//! These catch the cheap mistakes: a migration file renamed or deleted, a
//! version number reused, or the E-track migrations silently dropped. The
//! heavier "apply and inspect" checks run against a live PostgreSQL in CI.

use gs_store::migrate::{MIGRATOR, expected_schema_version};

#[test]
fn schema_version_covers_the_e_track_and_the_console() {
    // 0001 init, 0003 engine state, 0004 history, 0005 edge, 0006 console accounts.
    assert_eq!(expected_schema_version(), 6);
}

#[test]
fn versions_are_strictly_increasing() {
    let versions: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    let mut sorted = versions.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        versions.len(),
        sorted.len(),
        "duplicate migration versions: {versions:?}"
    );
    let mut prev = -1;
    for v in versions {
        assert!(v > prev, "versions must be strictly increasing");
        prev = v;
    }
}

#[test]
fn e_track_migrations_are_present() {
    let names: Vec<String> = MIGRATOR.iter().map(|m| m.description.to_string()).collect();
    for expected in ["engine state", "history", "edge"] {
        assert!(
            names.iter().any(|n| n.contains(expected)),
            "missing migration describing `{expected}`; have {names:?}"
        );
    }
}
