#!/usr/bin/env python3
"""Choose a new release review branch without replacing previous attempts."""
import argparse
import json
import os
import re
import subprocess
import sys


def select_branch(version, refs, pull_requests, run_id, attempt):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("Expected a stable major.minor.patch version")
    branch = f"release/v{version}"
    if f"refs/tags/v{version}" in refs:
        raise ValueError(f"Tag v{version} already exists; choose a new release version.")
    for pr in pull_requests:
        head = pr["headRefName"]
        if head == branch or head.startswith(f"{branch}-prep-"):
            raise ValueError(f"Version v{version} already has an open preparation PR: {pr['url']}")
    if f"refs/heads/{branch}" not in refs:
        return branch
    if not re.fullmatch(r"[0-9]+", run_id) or not re.fullmatch(r"[0-9]+", attempt):
        raise ValueError("GITHUB_RUN_ID and GITHUB_RUN_ATTEMPT are required to retry preparation")
    retry = f"{branch}-prep-{run_id}-{attempt}"
    if f"refs/heads/{retry}" in refs:
        raise ValueError(f"Preparation branch {retry} already exists; start a new workflow attempt.")
    return retry


def inspect_branch(version, run_id, attempt):
    # Inspect all refs so annotated tags and earlier retry branches are covered.
    result = subprocess.run(
        ["git", "ls-remote", "--refs", "origin"],
        check=True, capture_output=True, text=True,
    )
    refs = {line.split()[1] for line in result.stdout.splitlines()}
    result = subprocess.run(
        ["gh", "pr", "list", "--state", "open", "--base", "main", "--limit", "1000",
         "--json", "headRefName,url"],
        check=True, capture_output=True, text=True,
    )
    return select_branch(version, refs, json.loads(result.stdout), run_id, attempt)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    args = parser.parse_args()
    try:
        print(inspect_branch(args.version, os.environ.get("GITHUB_RUN_ID", ""),
                             os.environ.get("GITHUB_RUN_ATTEMPT", "")))
    except (ValueError, subprocess.CalledProcessError) as error:
        print(str(error), file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr.strip(), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
