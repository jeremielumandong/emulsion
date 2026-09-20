#!/usr/bin/env python3
"""Check the local GPUI patches and preserved license texts (Python 3.11+)."""

import hashlib
import json
from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parent.parent
VENDOR = ROOT / "vendor/gpui"


def read_toml(path):
    with path.open("rb") as source:
        return tomllib.load(source)


def validate(root=ROOT):
    vendor = root / "vendor/gpui"
    inventory = json.loads((vendor / "UPSTREAM.json").read_text())
    manifest = read_toml(root / "Cargo.toml")
    lock = read_toml(root / "Cargo.lock")
    errors = []

    def check(condition, message):
        if not condition:
            errors.append(message)

    names = {entry["name"] for entry in inventory}
    check(len(inventory) == len(names) == 29, "expected 29 unique GPUI packages")
    patches = manifest.get("patch", {}).get("crates-io", {})
    patched_names = {name for name in patches if name.startswith("gpui")}
    locked = [p for p in lock["package"] if p["name"].startswith("gpui")]
    check(patched_names == names, "GPUI patch names differ from UPSTREAM.json")
    check({p["name"] for p in locked} == names, "GPUI lock names differ from UPSTREAM.json")
    check(len(locked) == len(names), "GPUI lock contains missing or duplicate versions")
    dirs = {p.name for p in vendor.iterdir() if p.is_dir() and p.name.startswith("gpui")}
    check(dirs == names, "vendored package directories differ from UPSTREAM.json")

    workspace = manifest["workspace"]
    members = {
        path.resolve()
        for pattern in workspace.get("members", [])
        for path in root.glob(pattern)
    }
    excludes = {
        path.resolve()
        for pattern in workspace.get("exclude", [])
        for path in root.glob(pattern)
    }
    for entry in inventory:
        name = entry["name"]
        directory = vendor / name
        cargo_file = directory / "Cargo.toml"
        if not cargo_file.is_file():
            errors.append(f"{name}: missing Cargo.toml")
            continue
        package = read_toml(cargo_file)["package"]
        check(package["name"] == name, f"{name}: manifest name changed")
        check(package["version"] == entry["version"], f"{name}: manifest version differs from upstream")
        check(package.get("license") == entry["license"], f"{name}: upstream license declaration changed")
        patch = patches.get(name, {})
        patch_path = patch.get("path") if isinstance(patch, dict) else None
        check(bool(patch_path) and (root / patch_path).resolve() == directory.resolve(),
              f"{name}: patch does not point to its vendored directory")
        matching = [p for p in locked if p["name"] == name]
        for package_lock in matching:
            check(package_lock["version"] == entry["version"], f"{name}: lock version differs from upstream")
            check("source" not in package_lock and "checksum" not in package_lock,
                  f"{name}: lock still resolves a registry/git dependency")
        check(directory.resolve() in excludes,
              f"{name}: must be explicitly excluded from the application workspace")
        check(directory.resolve() not in members - excludes,
              f"{name}: vendored dependency became an application workspace member")
        for relative, expected in entry["upstream_license_files"].items():
            path = directory / relative
            check(path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == expected,
                  f"{name}/{relative}: upstream license missing or changed")
        check((directory / "LICENSE-APACHE").is_file(), f"{name}: missing Apache license text")

    original = vendor / "gpui-base/LICENSE-APACHE"
    for name in ("gpui-kit", "gpui-kit-assets"):
        supplement = vendor / name / "LICENSE-APACHE"
        check(original.is_file() and supplement.is_file()
              and supplement.read_bytes() == original.read_bytes(),
              f"{name}: supplemental Apache license differs from its same-revision source")
    microsoft = vendor / "licenses/MICROSOFT-TERMINAL-MIT.txt"
    check(microsoft.is_file() and hashlib.sha256(microsoft.read_bytes()).hexdigest()
          == "5d177f23ecfeb0ea8e050b6a5a16355e1ae9a0b286436ca8f83ed08b3795be6b",
          "Microsoft Terminal supplemental MIT license missing or changed")
    for path in (root / "THIRD_PARTY_NOTICES.md", vendor / "LICENSING.md"):
        check(path.is_file() and path.stat().st_size > 0, f"missing attribution document: {path}")
    return errors


def main():
    try:
        errors = validate()
    except (OSError, ValueError, KeyError, TypeError) as error:
        errors = [f"cannot read vendor metadata: {error}"]
    if errors:
        print("GPUI vendor validation failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print("GPUI vendor validation passed: 29 local packages, matching lock/patches, "
          "workspace exclusions, and preserved license texts.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
