#!/usr/bin/env python3
"""Check release version increments and preservation of dependency locks."""
import importlib.util
from pathlib import Path
import tempfile
import sys
import tomllib
import unittest

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location('bump_version', Path(__file__).with_name('bump-version.py'))
bump = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bump)

class VersionTests(unittest.TestCase):
    def test_increments(self):
        self.assertEqual(bump.next_version('0.0.1', 'patch'), '0.0.2')
        self.assertEqual(bump.next_version('1.2.9', 'minor'), '1.3.0')
        self.assertEqual(bump.next_version('1.2.9', 'major'), '2.0.0')

    def test_rejects_ambiguous_versions(self):
        for version in ['v1.2.3', '1.2.3-rc.1', '01.2.3', '1.2', '1.2.3+meta']:
            with self.subTest(version=version), self.assertRaises(ValueError):
                bump.next_version(version, 'patch')

    def test_updates_only_workspace_versions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = '[workspace]\nmembers = ["crates/*"]\n[workspace.package]\nversion = "0.0.1" # release\n[workspace.dependencies]\nexternal = "0.0.1"\n'
            lock = 'version = 4\n\n[[package]]\nname = "app"\nversion = "0.0.1"\ndependencies = ["external"]\n\n[[package]]\nname = "external"\nversion = "0.0.1"\nsource = "registry+https://example.invalid/index"\nchecksum = "untouched"\n'
            (root / 'Cargo.toml').write_text(manifest)
            (root / 'Cargo.lock').write_text(lock)
            (root / 'crates/app').mkdir(parents=True)
            (root / 'crates/app/Cargo.toml').write_text('[package]\nname = "app"\nversion.workspace = true\n')
            version, updated, locked = bump.prepare(root, 'patch')
            self.assertEqual(version, '0.0.2')
            self.assertEqual(tomllib.loads(updated)['workspace']['dependencies']['external'], '0.0.1')
            before = tomllib.loads(lock)['package']
            after = tomllib.loads(locked)['package']
            self.assertEqual(after[0]['version'], '0.0.2')
            self.assertEqual(after[1], before[1])
            self.assertIn('# release', updated)
            self.assertEqual((root / 'Cargo.toml').read_text(), manifest)
            self.assertEqual((root / 'Cargo.lock').read_text(), lock)
            (root / 'Cargo.lock').write_text(lock.replace('name = "app"', 'name = "missing"'))
            with self.assertRaisesRegex(ValueError, 'Missing lockfile entries'):
                bump.prepare(root, 'patch')
            (root / 'Cargo.lock').write_text(lock.replace('version = "0.0.1"', 'version = "0.0.0"', 1))
            with self.assertRaisesRegex(ValueError, 'Unexpected lockfile version'):
                bump.prepare(root, 'patch')

if __name__ == '__main__':
    unittest.main()
