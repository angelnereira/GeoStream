#!/usr/bin/env python3
"""Check that all JSON files under contracts/ are syntactically valid JSON."""

import json
import os
import sys
from pathlib import Path

def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    contracts_dir = repo_root / "contracts"

    if not contracts_dir.exists():
        print(f"ERROR: {contracts_dir} not found", file=sys.stderr)
        return 1

    errors = []
    checked = 0

    for root, dirs, files in os.walk(contracts_dir):
        for f in files:
            if not f.endswith(".json"):
                continue
            p = Path(root) / f
            rel_path = p.relative_to(repo_root)
            checked += 1
            try:
                with open(p, "r", encoding="utf-8") as fp:
                    json.load(fp)
            except Exception as e:
                errors.append(f"{rel_path}: {e}")

    if errors:
        print("FAIL: Invalid JSON files found:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print(f"OK: All {checked} JSON files under contracts/ are valid.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
