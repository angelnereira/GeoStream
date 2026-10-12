#!/usr/bin/env python3
"""Check that all operationIds and scopes in docs/06_IDENTITY_AUTH.md §8.2 match OpenAPI x-gs-auth."""

import re
import sys
from pathlib import Path

def normalize_doc_scope(val: str) -> str:
    val = val.strip()
    if val == "✗":
        return "forbidden"
    if val in ("—", "sin autenticación"):
        return "none"
    m = re.search(r"`([a-zA-Z0-9_:]+)`", val)
    if m:
        return m.group(1)
    return val

def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    doc_file = repo_root / "docs" / "06_IDENTITY_AUTH.md"
    openapi_file = repo_root / "contracts" / "openapi" / "geostream.v1.yaml"

    if not doc_file.exists():
        print(f"ERROR: {doc_file} not found", file=sys.stderr)
        return 1
    if not openapi_file.exists():
        print(f"ERROR: {openapi_file} not found", file=sys.stderr)
        return 1

    doc_text = doc_file.read_text(encoding="utf-8")
    if "### 8.2 Matriz REST" not in doc_text:
        print("ERROR: Section 8.2 not found in 06_IDENTITY_AUTH.md", file=sys.stderr)
        return 1

    sec82 = doc_text.split("### 8.2 Matriz REST")[1].split("### 8.3")[0]
    doc_ops = {}
    for line in sec82.splitlines():
        line = line.strip()
        if not line.startswith("|") or "operationId" in line or "---" in line:
            continue
        cols = [c.strip() for c in line.split("|")[1:-1]]
        if len(cols) >= 3:
            op_col = cols[0]
            app_scope = cols[1]
            dev_scope = cols[2]
            op_names = re.findall(r"`([a-zA-Z0-9]+)`", op_col)
            for name in op_names:
                doc_ops[name] = {
                    "app": normalize_doc_scope(app_scope),
                    "dev": normalize_doc_scope(dev_scope),
                }

    # Parse OpenAPI operations
    openapi_text = openapi_file.read_text(encoding="utf-8")
    openapi_ops = {}
    curr_op = None

    for line in openapi_text.splitlines():
        op_m = re.match(r"^\s*operationId:\s*([a-zA-Z0-9]+)", line)
        if op_m:
            curr_op = op_m.group(1)
            openapi_ops[curr_op] = {}
        if curr_op:
            auth_m = re.search(r"x-gs-auth:\s*\{\s*application:\s*([^,]+),\s*device:\s*([^}]+)\}", line)
            if auth_m:
                openapi_ops[curr_op]["app"] = auth_m.group(1).strip().strip("'\"")
                openapi_ops[curr_op]["dev"] = auth_m.group(2).strip().strip("'\"")

    errors = []

    # Verify doc operations vs OpenAPI
    for op, doc_data in sorted(doc_ops.items()):
        if op not in openapi_ops:
            errors.append(f"Operation `{op}` in 06_IDENTITY_AUTH.md §8.2 does not exist in OpenAPI")
            continue
        o_data = openapi_ops[op]
        if doc_data["app"] != o_data.get("app"):
            errors.append(
                f"Operation `{op}` application scope mismatch: "
                f"doc='{doc_data['app']}', openapi='{o_data.get('app')}'"
            )
        if doc_data["dev"] != o_data.get("dev"):
            errors.append(
                f"Operation `{op}` device scope mismatch: "
                f"doc='{doc_data['dev']}', openapi='{o_data.get('dev')}'"
            )

    # Check for any REST operations in OpenAPI that are missing in doc
    # (Excluding WebSocket upgrade 'openStream' and outgoing webhooks 'on*Event')
    for op, o_data in sorted(openapi_ops.items()):
        if op.startswith("on") and op.endswith("Event"):
            continue
        if op == "openStream":
            continue
        if op not in doc_ops:
            errors.append(f"OpenAPI operation `{op}` is not documented in 06_IDENTITY_AUTH.md §8.2")

    if errors:
        print("FAIL: Auth matrix inconsistencies found:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print(f"OK: All {len(doc_ops)} REST operations and auth scopes match perfectly with OpenAPI.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
