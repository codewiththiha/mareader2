#!/usr/bin/env python3
"""Map a Git diff range to the Cargo packages whose tests the quick CI gate runs.

Prints Cargo arguments (for example ``--package mareader-ui``), ``--workspace``
for workspace-wide/tooling changes, or an empty string when only prose changed.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROSE_ROOTS = ("docs/",)
PROSE_SUFFIXES = (".md",)
GLOBAL_FILES = {
    "Cargo.lock",
    "Cargo.toml",
    "tools/changed_crates.py",
}
GLOBAL_ROOTS = (
    ".cargo/",
    ".github/",
    "xtask/",
    "tools/",
    "parity/",
    "assets/",
)
GLOBAL_PREFIXES = ("rust-toolchain",)


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def is_prose(path: str) -> bool:
    return path.startswith(PROSE_ROOTS) or path.endswith(PROSE_SUFFIXES)


def is_global(path: str) -> bool:
    return (
        path in GLOBAL_FILES
        or path.startswith(GLOBAL_ROOTS)
        or Path(path).name.startswith(GLOBAL_PREFIXES)
    )


def package_roots() -> list[tuple[Path, str]]:
    raw = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    metadata = json.loads(raw)
    members = set(metadata["workspace_members"])
    packages = [package for package in metadata["packages"] if package["id"] in members]
    roots = [
        (Path(package["manifest_path"]).resolve().parent, package["name"])
        for package in packages
    ]
    return sorted(roots, key=lambda item: len(item[0].parts), reverse=True)


def changed_paths(diff_range: str) -> list[str]:
    try:
        output = git("diff", "--name-only", "--diff-filter=ACDMRTUXB", diff_range, "--")
    except subprocess.CalledProcessError:
        # New branches have an all-zero `before` SHA. Compare them with main.
        output = git("diff", "--name-only", "--diff-filter=ACDMRTUXB", "origin/main...HEAD", "--")
    return sorted(set(line for line in output.splitlines() if line))


def cargo_args(diff_range: str) -> list[str]:
    changed = [path for path in changed_paths(diff_range) if not is_prose(path)]
    if not changed:
        return []
    if any(is_global(path) for path in changed):
        return ["--workspace"]

    roots = package_roots()
    selected: set[str] = set()
    for relative in changed:
        absolute = (ROOT / relative).resolve()
        package = next(
            (name for root, name in roots if absolute == root or root in absolute.parents),
            None,
        )
        if package is None:
            # Unknown build/tool inputs are safer as a full-workspace gate.
            return ["--workspace"]
        selected.add(package)

    if len(selected) == len(roots):
        return ["--workspace"]
    args: list[str] = []
    for package in sorted(selected):
        args.extend(("--package", package))
    return args


def main() -> int:
    diff_range = sys.argv[1] if len(sys.argv) > 1 else "origin/main...HEAD"
    try:
        args = cargo_args(diff_range)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"changed_crates: {error}", file=sys.stderr)
        return 1
    print(" ".join(args))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
