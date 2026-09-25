#!/usr/bin/env python3
"""Bump the shared release version without upgrading locked dependencies."""
import argparse
from pathlib import Path
import re
import tomllib


def next_version(version, bump):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError(f"Expected a stable major.minor.patch version, got {version!r}")
    major, minor, patch = map(int, version.split("."))
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    if bump == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise ValueError(f"Unknown version bump: {bump}")


def prepare(root, bump):
    manifest = (root / "Cargo.toml").read_text()
    workspace = tomllib.loads(manifest)["workspace"]
    old = workspace["package"]["version"]
    new = next_version(old, bump)
    names = set()
    for pattern in workspace["members"]:
        for member in root.glob(pattern):
            package = tomllib.loads((member / "Cargo.toml").read_text())["package"]
            if package["version"] != {"workspace": True}:
                raise ValueError(f"{member} does not inherit the workspace version")
            names.add(package["name"])
    if not names:
        raise ValueError("No workspace packages found")

    section = re.search(r"(?ms)^\[workspace\.package\]\n.*?(?=^\[|\Z)", manifest)
    if not section:
        raise ValueError("Workspace package section not found")
    updated, count = re.subn(r'(?m)^version\s*=\s*"' + re.escape(old) + r'"',
                             f'version = "{new}"', section[0])
    if count != 1:
        raise ValueError("Expected one workspace version declaration")
    manifest = manifest[:section.start()] + updated + manifest[section.end():]

    lock = (root / "Cargo.lock").read_text()
    seen = set()

    def update_package(match):
        block = match[0]
        package = tomllib.loads(block)["package"][0]
        if package["name"] not in names or "source" in package:
            return block
        if package["name"] in seen or package["version"] != old:
            raise ValueError(f"Unexpected lockfile version for {package['name']}")
        seen.add(package["name"])
        return re.sub(r'(?m)^version = "' + re.escape(old) + r'"$',
                      f'version = "{new}"', block, count=1)

    lock = re.sub(r"(?ms)^\[\[package\]\]\n.*?(?=^\[\[package\]\]|\Z)", update_package, lock)
    if seen != names:
        raise ValueError(f"Missing lockfile entries: {sorted(names - seen)}")
    # Cargo may qualify dependency names with versions when names collide.
    for name in names:
        lock = lock.replace(f'"{name} {old}"', f'"{name} {new}"')
    tomllib.loads(manifest)
    tomllib.loads(lock)
    return new, manifest, lock


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bump", choices=["patch", "minor", "major"])
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    version, manifest, lock = prepare(root, args.bump)
    if not args.dry_run:
        (root / "Cargo.toml").write_text(manifest)
        (root / "Cargo.lock").write_text(lock)
    print(version)


if __name__ == "__main__":
    main()
