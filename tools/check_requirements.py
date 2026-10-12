#!/usr/bin/env python3
"""Check that all requirement IDs across docs exist in 03_REQUIREMENTS.md and TRACEABILITY.md."""

import os
import re
import sys
from pathlib import Path

REQ_PATTERN = re.compile(r"\b((?:FR|NFR)-[A-Z]+-[0-9]{3})\b")

def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    req_file = repo_root / "docs" / "03_REQUIREMENTS.md"
    trace_file = repo_root / "docs" / "TRACEABILITY.md"

    if not req_file.exists():
        print(f"ERROR: {req_file} not found", file=sys.stderr)
        return 1
    if not trace_file.exists():
        print(f"ERROR: {trace_file} not found", file=sys.stderr)
        return 1

    req_text = req_file.read_text(encoding="utf-8")
    defined_reqs = set(REQ_PATTERN.findall(req_text))

    trace_text = trace_file.read_text(encoding="utf-8")
    traced_reqs = set(REQ_PATTERN.findall(trace_text))

    errors = []

    # Check 03 vs TRACEABILITY
    in_03_not_trace = defined_reqs - traced_reqs
    if in_03_not_trace:
        errors.append(f"In 03_REQUIREMENTS.md but not in TRACEABILITY.md: {sorted(in_03_not_trace)}")

    in_trace_not_03 = traced_reqs - defined_reqs
    if in_trace_not_03:
        errors.append(f"In TRACEABILITY.md but not in 03_REQUIREMENTS.md: {sorted(in_trace_not_03)}")

    # Scan normative docs for any unknown requirement IDs
    docs_dir = repo_root / "docs"
    for root, dirs, files in os.walk(docs_dir):
        rel_root = Path(root).relative_to(repo_root)
        if "proposals" in rel_root.parts:
            continue

        for f in files:
            if not f.endswith(".md"):
                continue
            p = Path(root) / f
            rel_path = p.relative_to(repo_root)
            text = p.read_text(encoding="utf-8")
            found = set(REQ_PATTERN.findall(text))
            unknown = found - defined_reqs
            if unknown:
                errors.append(f"{rel_path}: references unknown requirement IDs: {sorted(unknown)}")

    if errors:
        print("FAIL: Requirement consistency errors found:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print(
        f"OK: All {len(defined_reqs)} requirement IDs verified across 03_REQUIREMENTS.md, "
        f"TRACEABILITY.md, and all normative docs."
    )
    return 0

if __name__ == "__main__":
    sys.exit(main())
