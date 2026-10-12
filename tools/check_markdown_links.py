#!/usr/bin/env python3
"""Check that relative markdown links across the repository point to existing files."""

import os
import re
import sys
from pathlib import Path

LINK_PATTERN = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")

def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    broken = []
    total_links = 0

    for root, dirs, files in os.walk(repo_root):
        rel_root = Path(root).relative_to(repo_root)
        parts = rel_root.parts
        # Skip git, target, caches
        if any(p.startswith(".") or p in ("target", "node_modules") for p in parts):
            continue

        for f in files:
            if not f.endswith(".md"):
                continue
            file_path = Path(root) / f
            rel_file = file_path.relative_to(repo_root)
            try:
                content = file_path.read_text(encoding="utf-8")
            except Exception as e:
                broken.append((str(rel_file), f"Read error: {e}", ""))
                continue

            # Strip code blocks to avoid false positives in code examples
            stripped = re.sub(r"```[\s\S]*?```", "", content)

            for text, link in LINK_PATTERN.findall(stripped):
                link = link.strip()
                # Skip external links and internal anchors
                if link.startswith(("#", "http://", "https://", "mailto:")):
                    continue
                # Skip conversation:// or custom schema
                if "://" in link:
                    continue

                total_links += 1
                target = link.split("#")[0].strip()
                if not target:
                    continue

                target_path = (file_path.parent / target).resolve()
                if not target_path.exists():
                    broken.append((str(rel_file), link, str(target_path)))

    if broken:
        print("FAIL: Broken markdown links detected:")
        for src, link, resolved in broken:
            print(f"  - In {src}: `{link}` (resolved: {resolved})")
        return 1

    print(f"OK: All {total_links} relative markdown links successfully verified.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
