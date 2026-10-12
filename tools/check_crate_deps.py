#!/usr/bin/env python3
"""Check crate dependency rules defined in docs/04_ARCHITECTURE.md §4.1."""

import json
import subprocess
import sys
from pathlib import Path

ALLOWED_INTERNAL = {
    "gs-domain": set(),
    "gs-protocol": {"gs-domain"},
    "gs-config": {"gs-domain"},
    "gs-observability": {"gs-domain"},
    "gs-auth": {"gs-domain", "gs-config"},
    "gs-store": {"gs-domain"},
    "gs-ephemeral": {"gs-domain", "gs-protocol"},
    "gs-bus": {"gs-domain"},
    "gs-geo": {"gs-domain"},
    "gs-service": {"gs-auth", "gs-store", "gs-ephemeral", "gs-bus", "gs-protocol", "gs-domain"},
    "gs-api": {"gs-service"},
    "gs-stream": {"gs-service", "gs-geo"},
    "gs-worker": {"gs-service"},
    "geostream": {"gs-api", "gs-stream", "gs-worker"},
    "gs-testkit": None,  # test harness can depend on necessary components
    "gs-devclient": None,
    "gs-loadgen": None,
}

FORBIDDEN_EXTERNAL = {
    "gs-domain": {"tokio", "sqlx", "axum", "redis"},
}

def main() -> int:
    try:
        output = subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            stderr=subprocess.PIPE,
        )
    except Exception as e:
        print(f"ERROR: Failed to run cargo metadata: {e}", file=sys.stderr)
        return 1

    metadata = json.loads(output)
    errors = []

    for pkg in metadata.get("packages", []):
        name = pkg["name"]
        # Normal (runtime) dependencies
        deps = [
            d["name"]
            for d in pkg.get("dependencies", [])
            if d.get("kind") is None or d.get("kind") == "normal"
        ]
        internal_deps = {d for d in deps if d.startswith("gs-") or d == "geostream"}
        external_deps = set(deps) - internal_deps

        # 1. Check allowed internal dependencies
        if name in ALLOWED_INTERNAL and ALLOWED_INTERNAL[name] is not None:
            allowed = ALLOWED_INTERNAL[name]
            illegal_internal = internal_deps - allowed
            if illegal_internal:
                errors.append(
                    f"{name}: illegal internal dependencies {sorted(illegal_internal)} "
                    f"(allowed: {sorted(allowed)})"
                )

        # 2. Check forbidden external dependencies (gs-domain rules)
        if name in FORBIDDEN_EXTERNAL:
            forbidden = FORBIDDEN_EXTERNAL[name]
            illegal_external = external_deps & forbidden
            if illegal_external:
                errors.append(
                    f"{name}: forbidden external dependencies {sorted(illegal_external)}"
                )

        # 3. sqlx rule: only gs-store speaks with PostgreSQL
        if "sqlx" in external_deps and name not in {
            "gs-store",
            "gs-testkit",
            "gs-loadgen",
            "gs-devclient",
        }:
            errors.append(f"{name}: depends on sqlx directly, but only gs-store is allowed")

        # 4. redis rule: only gs-ephemeral and gs-bus speak with Redis/Valkey
        if "redis" in external_deps and name not in {
            "gs-ephemeral",
            "gs-bus",
            "gs-testkit",
            "gs-loadgen",
            "gs-devclient",
        }:
            errors.append(
                f"{name}: depends on redis directly, but only gs-ephemeral and gs-bus are allowed"
            )

        # 5. gs-api and gs-stream must not access gs-store, gs-ephemeral, gs-bus directly
        if name in {"gs-api", "gs-stream"}:
            forbidden_crates = internal_deps & {"gs-store", "gs-ephemeral", "gs-bus"}
            if forbidden_crates:
                errors.append(
                    f"{name}: directly accesses {sorted(forbidden_crates)}; must use gs-service"
                )

        # 6. gs-observability: gs-domain must not depend on it
        if name == "gs-domain" and "gs-observability" in internal_deps:
            errors.append("gs-domain must not depend on gs-observability")

    if errors:
        print("FAIL: Crate dependency rules violated:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print("OK: All crate dependency rules verified according to 04_ARCHITECTURE.md §4.1.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
