#!/usr/bin/env python3
"""Check that all GS_* configuration keys and tenant policy keys cited across docs/ exist in docs/17_CONFIGURATION.md."""

import os
import re
import sys
from pathlib import Path

def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    doc17_file = repo_root / "docs" / "17_CONFIGURATION.md"

    if not doc17_file.exists():
        print(f"ERROR: {doc17_file} not found", file=sys.stderr)
        return 1

    doc17 = doc17_file.read_text(encoding="utf-8")

    # 1. Server config keys from tables
    server_keys = set(re.findall(r"\|\s*`([A-Z0-9_]+)`\s*\|", doc17))
    server_keys = {k for k in server_keys if k.startswith("GS_")}

    # Documented integrator variables from section 6
    integrator_keys = {"GS_API_KEY", "GS_BASE_URL"}

    # 2. Policy keys from section 4
    if "## 4. Política por tenant" not in doc17:
        print("ERROR: Section 4 not found in 17_CONFIGURATION.md", file=sys.stderr)
        return 1
    sec4 = doc17.split("## 4. Política por tenant")[1].split("## 5.")[0]
    policy_keys = set()
    for line in sec4.splitlines():
        m = re.match(r"^\|\s*`([a-z0-9_]+)`\s*\|", line.strip())
        if m:
            policy_keys.add(m.group(1))

    # Scan normative documentation
    docs_dir = repo_root / "docs"
    errors = []

    for root, dirs, files in os.walk(docs_dir):
        # Exclude proposals and templates as they are non-normative
        rel_root = Path(root).relative_to(repo_root)
        if "proposals" in rel_root.parts or "templates" in rel_root.parts:
            continue

        for f in files:
            if not f.endswith(".md"):
                continue
            p = Path(root) / f
            rel_path = p.relative_to(repo_root)
            text = p.read_text(encoding="utf-8")

            # Check GS_* occurrences
            # Pattern matches GS_... and captures optional trailing underscore or asterisk
            for match in re.finditer(r"\b(GS_[A-Z0-9_]+)(\*?)", text):
                token = match.group(1)
                has_star = bool(match.group(2))

                # Allow generic references: GS_ or GS_<CLAVE>
                if token in ("GS_", "GS_CONFIG"):
                    continue

                # Secret _FILE variant
                if token.endswith("_FILE"):
                    base = token[:-5]
                    if base in server_keys:
                        continue

                # Wildcard family reference like GS_REST_RATE_LIMIT_* or GS_RETENTION_*
                if has_star or token.endswith("_"):
                    prefix = token if token.endswith("_") else token + "_"
                    if any(k.startswith(prefix) for k in server_keys):
                        continue

                if token not in server_keys and token not in integrator_keys:
                    errors.append(f"{rel_path}: Unknown server configuration key `{token}`")

    if errors:
        print("FAIL: Configuration key inconsistencies found:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print(
        f"OK: All GS_* keys and policy references in docs are valid against 17_CONFIGURATION.md "
        f"({len(server_keys)} server keys, {len(policy_keys)} policy keys)."
    )
    return 0

if __name__ == "__main__":
    sys.exit(main())
