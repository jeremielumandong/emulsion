#!/usr/bin/env python3
"""Check packaged notices and reject incomplete license manifests."""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent


class LicenseStagingTests(unittest.TestCase):
    def test_manifest_covers_vendor_notices(self):
        files = [
            line
            for line in (ROOT / "packaging/license-files.txt").read_text().splitlines()
            if line and not line.startswith("#")
        ]
        self.assertEqual(len(files), len(set(files)), "duplicate notice")
        required = {"LICENSE", "THIRD_PARTY_NOTICES.md"}
        for path in (ROOT / "vendor/gpui").rglob("*"):
            if path.is_file() and (
                path.name.startswith(("LICENSE", "NOTICE", "COPYING"))
                or path.name in ("LICENSING.md", "UPSTREAM.json", "EMULSION_CHANGES.md")
                or "licenses" in path.relative_to(ROOT / "vendor/gpui").parts
            ):
                required.add(path.relative_to(ROOT).as_posix())
        self.assertFalse(required - set(files), f"unpackaged notices: {required - set(files)}")
        with tempfile.TemporaryDirectory(prefix="emulsion-licenses-") as temporary:
            destination = Path(temporary) / "licenses with spaces"
            subprocess.run(
                ["bash", str(ROOT / "scripts/stage-licenses.sh"), str(destination)],
                check=True,
            )
            for file in files:
                self.assertEqual((ROOT / file).read_bytes(), (destination / file).read_bytes())

    def check_rejected_notice(self, contents):
        with tempfile.TemporaryDirectory(prefix="emulsion-missing-license-") as temporary:
            fixture = Path(temporary)
            (fixture / "scripts").mkdir()
            (fixture / "packaging").mkdir()
            shutil.copyfile(ROOT / "scripts/stage-licenses.sh", fixture / "scripts/stage-licenses.sh")
            (fixture / "packaging/license-files.txt").write_text("LICENSE\nNOTICE\n")
            (fixture / "LICENSE").write_text("valid license\n")
            if contents is not None:
                (fixture / "NOTICE").write_text(contents)
            destination = fixture / "output"
            result = subprocess.run(
                ["bash", str(fixture / "scripts/stage-licenses.sh"), str(destination)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("Required license or notice is missing or empty: NOTICE", result.stderr)
            self.assertFalse(destination.exists(), "incomplete notice set was staged")

    def test_missing_notice_fails_before_staging(self):
        self.check_rejected_notice(None)

    def test_empty_notice_fails_before_staging(self):
        self.check_rejected_notice("")


if __name__ == "__main__":
    unittest.main()
