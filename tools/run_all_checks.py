#!/usr/bin/env python3
"""Run all GeoStream consistency and lint checks."""

import subprocess
import sys
from pathlib import Path

CHECKS = [
    ("check_json_validity.py", "Contracts JSON Validity"),
    ("check_crate_deps.py", "Crate Dependency Rules (04 §4.1)"),
    ("check_error_catalog.py", "ErrorCode Catalog Consistency"),
    ("check_config_keys.py", "Configuration Keys Consistency (17)"),
    ("check_requirements.py", "Requirement IDs and Traceability (03, TRACEABILITY)"),
    ("check_auth_matrix.py", "Auth Matrix Consistency (06 §8.2 vs OpenAPI)"),
    ("check_markdown_links.py", "Markdown Relative Links"),
]

def main() -> int:
    tools_dir = Path(__file__).resolve().parent
    failed = []

    print("=" * 70)
    print("Running GeoStream Consistency Checks (tools/)")
    print("=" * 70)

    for script, description in CHECKS:
        script_path = tools_dir / script
        print(f"\n--- [{description}] ({script}) ---")
        res = subprocess.run([sys.executable, str(script_path)])
        if res.returncode != 0:
            failed.append((script, description))

    print("\n" + "=" * 70)
    if failed:
        print("FAIL: The following checks failed:")
        for script, description in failed:
            print(f"  - {description} ({script})")
        print("=" * 70)
        return 1

    print("SUCCESS: All consistency checks passed!")
    print("=" * 70)
    return 0

if __name__ == "__main__":
    sys.exit(main())
