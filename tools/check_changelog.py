#!/usr/bin/env python3
"""Check that CHANGELOG.md follows Keep a Changelog and 19_ENGINEERING_STANDARDS.md §8."""

import argparse
import re
import subprocess
import sys
from pathlib import Path

VALID_CATEGORIES = {
    "Added",
    "Changed",
    "Deprecated",
    "Removed",
    "Fixed",
    "Security",
    "Docs",
}

def check_changelog_format(repo_root: Path) -> list[str]:
    changelog_file = repo_root / "CHANGELOG.md"
    if not changelog_file.exists():
        return [f"{changelog_file} does not exist"]

    content = changelog_file.read_text(encoding="utf-8")
    errors = []

    if "keepachangelog.com" not in content.lower():
        errors.append("CHANGELOG.md must reference Keep a Changelog standard")

    if "## [Unreleased]" not in content:
        errors.append("CHANGELOG.md must contain a `## [Unreleased]` section")

    # Check subsection categories
    for line in content.splitlines():
        if line.startswith("### "):
            category = line.replace("### ", "").strip()
            # Category might have extra parenthetical notes e.g. "Changed (respecto a...)"
            base_category = category.split("(")[0].strip()
            if base_category not in VALID_CATEGORIES:
                errors.append(
                    f"Invalid category `### {category}`. "
                    f"Must be one of {sorted(VALID_CATEGORIES)}"
                )

    return errors

def check_pr_diff(repo_root: Path, base_ref: str) -> list[str]:
    """Check that CHANGELOG.md was touched in the PR diff."""
    try:
        diff_output = subprocess.check_output(
            ["git", "diff", "--name-only", f"{base_ref}...HEAD"],
            cwd=repo_root,
            stderr=subprocess.PIPE,
            text=True,
        )
        changed_files = diff_output.splitlines()
        if "CHANGELOG.md" not in changed_files:
            return ["CHANGELOG.md has not been updated in this PR"]
    except subprocess.CalledProcessError:
        # If git diff fails (e.g. shallow clone or invalid base), warn but don't hard crash
        print(f"Warning: Could not check git diff against {base_ref}", file=sys.stderr)
    return []

def main() -> int:
    parser = argparse.ArgumentParser(description="Validate CHANGELOG.md format and PR updates.")
    parser.add_argument("--base-ref", help="Git base reference to check PR diff (e.g. origin/main)")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent

    errors = check_changelog_format(repo_root)

    if args.base_ref:
        errors.extend(check_pr_diff(repo_root, args.base_ref))

    if errors:
        print("FAIL: CHANGELOG.md validation failed:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print("OK: CHANGELOG.md format complies with Keep a Changelog and 19 §8.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
