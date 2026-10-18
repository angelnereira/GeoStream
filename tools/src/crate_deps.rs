//! Crate dependency rules verification per docs/04_ARCHITECTURE.md §4.1.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub fn check_crate_deps(repo_root: &Path) -> Result<(), Vec<String>> {
    let output = match Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(repo_root)
        .output()
    {
        Ok(out) => out,
        Err(e) => return Err(vec![format!("Failed to run cargo metadata: {e}")]),
    };

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(vec![format!("cargo metadata exited with error: {err}")]);
    }

    let val: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("Failed to parse cargo metadata JSON: {e}")]),
    };

    let mut allowed_internal: BTreeMap<&str, Option<BTreeSet<&str>>> = BTreeMap::new();
    allowed_internal.insert("gs-domain", Some(BTreeSet::new()));
    allowed_internal.insert("gs-edge", Some(BTreeSet::from(["gs-domain"])));
    allowed_internal.insert("gs-pools", Some(BTreeSet::from(["gs-domain", "gs-geo"])));
    allowed_internal.insert("gs-protocol", Some(BTreeSet::from(["gs-domain"])));
    allowed_internal.insert("gs-config", Some(BTreeSet::from(["gs-domain"])));
    allowed_internal.insert("gs-observability", Some(BTreeSet::from(["gs-domain"])));
    allowed_internal.insert("gs-auth", Some(BTreeSet::from(["gs-domain", "gs-config"])));
    allowed_internal.insert("gs-store", Some(BTreeSet::from(["gs-domain"])));
    allowed_internal.insert("gs-bus", Some(BTreeSet::from(["gs-domain"])));
    allowed_internal.insert("gs-geo", Some(BTreeSet::from(["gs-domain"])));
    allowed_internal.insert("gs-track", Some(BTreeSet::from(["gs-domain", "gs-geo"])));
    allowed_internal.insert("gs-mobility", Some(BTreeSet::from(["gs-domain", "gs-geo"])));
    allowed_internal.insert(
        "gs-analytics",
        Some(BTreeSet::from(["gs-domain", "gs-geo", "gs-track"])),
    );
    allowed_internal.insert("gs-providers", Some(BTreeSet::from(["gs-domain"])));
    allowed_internal.insert(
        "gs-ephemeral",
        Some(BTreeSet::from(["gs-domain", "gs-protocol", "gs-mobility"])),
    );
    allowed_internal.insert(
        "gs-service",
        Some(BTreeSet::from([
            "gs-auth",
            "gs-store",
            "gs-ephemeral",
            "gs-bus",
            "gs-protocol",
            "gs-domain",
            "gs-track",
            "gs-mobility",
            "gs-analytics",
            "gs-providers",
            "gs-edge",
        ])),
    );
    allowed_internal.insert(
        "gs-api",
        Some(BTreeSet::from([
            "gs-service",
            "gs-geo",
            "gs-mobility",
            "gs-pools",
            "gs-protocol",
            "gs-track",
        ])),
    );
    allowed_internal.insert(
        "gs-stream",
        Some(BTreeSet::from(["gs-service", "gs-geo", "gs-track"])),
    );
    allowed_internal.insert("gs-worker", Some(BTreeSet::from(["gs-service"])));
    allowed_internal.insert(
        "geostream",
        Some(BTreeSet::from([
            "gs-api",
            "gs-stream",
            "gs-worker",
            "gs-config",
        ])),
    );
    allowed_internal.insert(
        "gs-client-core",
        Some(BTreeSet::from([
            "gs-domain",
            "gs-protocol",
            "gs-geo",
            "gs-track",
            "gs-edge",
        ])),
    );
    allowed_internal.insert(
        "gs-client-runtime",
        Some(BTreeSet::from(["gs-client-core"])),
    );
    allowed_internal.insert(
        "gs-ffi",
        Some(BTreeSet::from(["gs-client-core", "gs-client-runtime"])),
    );
    allowed_internal.insert(
        "gs-analytics",
        Some(BTreeSet::from(["gs-domain", "gs-geo"])),
    );
    allowed_internal.insert("gs-testkit", None);
    allowed_internal.insert("gs-devclient", None);
    allowed_internal.insert("gs-loadgen", None);
    allowed_internal.insert("geostream-tools", None);

    let pure_crates: BTreeSet<&str> = BTreeSet::from([
        "gs-domain",
        "gs-track",
        "gs-mobility",
        "gs-analytics",
        "gs-edge",
        "gs-client-core",
    ]);
    let forbidden_external_pure: BTreeSet<&str> =
        BTreeSet::from(["tokio", "sqlx", "axum", "redis", "reqwest"]);

    let server_crates: BTreeSet<&str> = BTreeSet::from([
        "gs-store",
        "gs-ephemeral",
        "gs-bus",
        "gs-auth",
        "gs-config",
        "gs-service",
        "gs-api",
        "gs-stream",
        "gs-worker",
        "geostream",
    ]);
    let client_only_crates: BTreeSet<&str> =
        BTreeSet::from(["gs-client-core", "gs-client-runtime", "gs-ffi"]);

    let mut errors = Vec::new();

    let packages = match val.get("packages").and_then(|p| p.as_array()) {
        Some(pkgs) => pkgs,
        None => return Err(vec!["No packages found in metadata".to_string()]),
    };

    for pkg in packages {
        let name = match pkg.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => continue,
        };

        let mut internal_deps = BTreeSet::new();
        let mut external_deps = BTreeSet::new();

        if let Some(deps) = pkg.get("dependencies").and_then(|d| d.as_array()) {
            for dep in deps {
                let kind = dep.get("kind").and_then(|k| k.as_str());
                if kind.is_some() && kind != Some("normal") {
                    continue; // Skip dev or build dependencies
                }
                if let Some(dep_name) = dep.get("name").and_then(|n| n.as_str()) {
                    if dep_name.starts_with("gs-") || dep_name == "geostream" {
                        internal_deps.insert(dep_name);
                    } else {
                        external_deps.insert(dep_name);
                    }
                }
            }
        }

        // 1. Allowed internal deps (`gs-observability` is permitted for any crate except `gs-domain` and `gs-edge`)
        if let Some(Some(allowed)) = allowed_internal.get(name) {
            let illegal: Vec<_> = internal_deps
                .difference(allowed)
                .copied()
                .filter(|&d| !(d == "gs-observability" && !matches!(name, "gs-domain" | "gs-edge")))
                .collect();
            if !illegal.is_empty() {
                errors.push(format!(
                    "{name}: illegal internal dependencies {illegal:?} (allowed: {allowed:?})"
                ));
            }
        }

        // 2. Forbidden external I/O deps on pure crates
        if pure_crates.contains(name) {
            let illegal: Vec<_> = external_deps
                .intersection(&forbidden_external_pure)
                .copied()
                .collect();
            if !illegal.is_empty() {
                errors.push(format!(
                    "{name}: forbidden external dependencies {illegal:?}"
                ));
            }
        }

        // 3. sqlx rule: only gs-store speaks with PostgreSQL
        if external_deps.contains("sqlx")
            && !matches!(
                name,
                "gs-store" | "gs-testkit" | "gs-loadgen" | "gs-devclient" | "geostream-tools"
            )
        {
            errors.push(format!(
                "{name}: depends on sqlx directly, but only gs-store is allowed"
            ));
        }

        // 4. redis rule: only gs-ephemeral and gs-bus speak with Redis/Valkey
        if external_deps.contains("redis")
            && !matches!(
                name,
                "gs-ephemeral"
                    | "gs-bus"
                    | "gs-testkit"
                    | "gs-loadgen"
                    | "gs-devclient"
                    | "geostream-tools"
            )
        {
            errors.push(format!(
                "{name}: depends on redis directly, but only gs-ephemeral and gs-bus are allowed"
            ));
        }

        // 5. gs-api and gs-stream must not access gs-store, gs-ephemeral, gs-bus directly
        if matches!(name, "gs-api" | "gs-stream") {
            let forbidden_crates = BTreeSet::from(["gs-store", "gs-ephemeral", "gs-bus"]);
            let direct: Vec<_> = internal_deps
                .intersection(&forbidden_crates)
                .copied()
                .collect();
            if !direct.is_empty() {
                errors.push(format!(
                    "{name}: directly accesses {direct:?}; must use gs-service"
                ));
            }
        }

        // 6. gs-observability: gs-domain must not depend on it
        if name == "gs-domain" && internal_deps.contains("gs-observability") {
            errors.push("gs-domain must not depend on gs-observability".to_string());
        }

        // 7. Server crates must not depend on gs-client-*
        if server_crates.contains(name) {
            let direct: Vec<_> = internal_deps
                .intersection(&client_only_crates)
                .copied()
                .collect();
            if !direct.is_empty() {
                errors.push(format!(
                    "{name}: server crate must not depend on client crates {direct:?}"
                ));
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    println!("OK: All crate dependency rules verified according to 04_ARCHITECTURE.md §4.1.");
    Ok(())
}
