#!/usr/bin/env python3
"""Check that ErrorCode catalog is identical across common.proto, OpenAPI, and docs/09_VALIDATION_ERRORS.md §6."""

import re
import sys
from pathlib import Path

def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent

    # 1. Parse common.proto
    proto_file = repo_root / "contracts" / "proto" / "geostream" / "v1" / "common.proto"
    if not proto_file.exists():
        print(f"ERROR: {proto_file} not found", file=sys.stderr)
        return 1
    proto_text = proto_file.read_text(encoding="utf-8")
    proto_match = re.search(r"enum ErrorCode\s*\{(.*?)\}", proto_text, re.DOTALL)
    if not proto_match:
        print("ERROR: enum ErrorCode not found in common.proto", file=sys.stderr)
        return 1
    proto_codes = set()
    for line in proto_match.group(1).splitlines():
        m = re.search(r"ERROR_CODE_([A-Z0-9_]+)\s*=\s*\d+;", line)
        if m and m.group(1) != "UNSPECIFIED":
            proto_codes.add(m.group(1))

    # 2. Parse OpenAPI ErrorCode enum
    openapi_file = repo_root / "contracts" / "openapi" / "geostream.v1.yaml"
    if not openapi_file.exists():
        print(f"ERROR: {openapi_file} not found", file=sys.stderr)
        return 1
    openapi_text = openapi_file.read_text(encoding="utf-8")
    openapi_match = re.search(
        r"ErrorCode:\s*\n\s*type:\s*string\s*\n\s*enum:\s*\n((?:\s*-\s*[A-Z0-9_]+\s*\n)+)",
        openapi_text,
    )
    if not openapi_match:
        print("ERROR: ErrorCode enum not found in OpenAPI spec", file=sys.stderr)
        return 1
    openapi_codes = set(re.findall(r"-\s*([A-Z0-9_]+)", openapi_match.group(1)))

    # 3. Parse docs/09_VALIDATION_ERRORS.md §6
    doc_file = repo_root / "docs" / "09_VALIDATION_ERRORS.md"
    if not doc_file.exists():
        print(f"ERROR: {doc_file} not found", file=sys.stderr)
        return 1
    doc_text = doc_file.read_text(encoding="utf-8")
    if "## 6. Catálogo de errores" not in doc_text:
        print("ERROR: Section 6 not found in 09_VALIDATION_ERRORS.md", file=sys.stderr)
        return 1
    sec6 = doc_text.split("## 6. Catálogo de errores")[1].split("## 7.")[0]
    doc_codes = set()
    for line in sec6.splitlines():
        m = re.match(r"^\|\s*`([A-Z0-9_]+)`\s*\|", line.strip())
        if m:
            doc_codes.add(m.group(1))

    errors = []
    if proto_codes != openapi_codes:
        diff_proto = proto_codes - openapi_codes
        diff_openapi = openapi_codes - proto_codes
        if diff_proto:
            errors.append(f"In common.proto but not in OpenAPI: {sorted(diff_proto)}")
        if diff_openapi:
            errors.append(f"In OpenAPI but not in common.proto: {sorted(diff_openapi)}")

    if proto_codes != doc_codes:
        diff_proto = proto_codes - doc_codes
        diff_doc = doc_codes - proto_codes
        if diff_proto:
            errors.append(f"In common.proto but not in 09_VALIDATION_ERRORS.md: {sorted(diff_proto)}")
        if diff_doc:
            errors.append(f"In 09_VALIDATION_ERRORS.md but not in common.proto: {sorted(diff_doc)}")

    if errors:
        print("FAIL: ErrorCode catalog mismatch detected:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print(f"OK: All {len(proto_codes)} error codes match across common.proto, OpenAPI, and 09_VALIDATION_ERRORS.md.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
