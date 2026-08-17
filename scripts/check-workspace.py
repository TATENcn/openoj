#!/usr/bin/env python3
"""Enforce OpenOJ workspace dependency direction without third-party Python packages."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
ALLOWED_WORKSPACE_EDGES = {
    "openoj-cli": {
        ("openoj-application", "normal"),
        ("openoj-domain", "normal"),
        ("openoj-protocol", "normal"),
        ("openoj-storage", "normal"),
    },
    "openoj-domain": set(),
    "openoj-judge-core": {
        ("openoj-application", "normal"),
        ("openoj-domain", "normal"),
        ("openoj-protocol", "dev"),
    },
    "openoj-judge-protocol": {
        ("openoj-domain", "normal"),
        ("openoj-protocol", "normal"),
    },
    "openoj-application": {
        ("openoj-domain", "normal"),
        ("openoj-protocol", "dev"),
    },
    "openoj-protocol": {
        ("openoj-domain", "normal"),
    },
    "openoj-storage": {
        ("openoj-application", "normal"),
        ("openoj-domain", "normal"),
        ("openoj-protocol", "normal"),
    },
}


def main() -> int:
    completed = subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--no-deps",
        ],
        cwd=REPOSITORY_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        print(completed.stderr, file=sys.stderr, end="")
        return completed.returncode

    metadata = json.loads(completed.stdout)
    workspace_members = set(metadata["workspace_members"])
    packages = {
        package["name"]: package
        for package in metadata["packages"]
        if package["id"] in workspace_members
    }

    if set(packages) != set(ALLOWED_WORKSPACE_EDGES):
        print(
            "ERROR: workspace package set changed; update the documented dependency policy",
            file=sys.stderr,
        )
        return 1

    workspace_names = set(packages)
    actual_edges: dict[str, set[tuple[str, str]]] = {}
    for package_name, package in packages.items():
        edges = set()
        for dependency in package["dependencies"]:
            dependency_name = dependency["name"]
            if dependency_name not in workspace_names:
                continue
            dependency_kind = dependency["kind"] or "normal"
            edges.add((dependency_name, dependency_kind))
        actual_edges[package_name] = edges

    if actual_edges != ALLOWED_WORKSPACE_EDGES:
        print("ERROR: workspace dependency direction changed", file=sys.stderr)
        print(f"expected: {ALLOWED_WORKSPACE_EDGES}", file=sys.stderr)
        print(f"actual:   {actual_edges}", file=sys.stderr)
        return 1

    print("Workspace dependency direction checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
