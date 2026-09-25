#!/usr/bin/env python3
"""Exercise the piped installer with local release fixtures; no network or user install."""
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
INSTALLER = ROOT / "site/public/install"
ASSET = "Emulsion-linux-x86_64.tar.gz"


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.marker = self.root / "installed"
        with tarfile.open(self.root / ASSET, "w:gz") as archive:
            for name, content in {
                "Emulsion.AppImage": b"fixture",
                "scripts/install-appimage.sh": b'#!/bin/bash\nset -eu\ntest -f "$1"\nprintf "%s\\n" "$@" > "$INSTALL_MARKER"\n',
            }.items():
                info = tarfile.TarInfo(name)
                info.size = len(content)
                archive.addfile(info, io.BytesIO(content))
        digest = hashlib.sha256((self.root / ASSET).read_bytes()).hexdigest()
        (self.root / (ASSET + ".sha256")).write_text(f"{digest}  {ASSET}\n")
        self.command("uname", '#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo "${TEST_ARCH:-x86_64}";; esac\n')
        self.command("curl", '''#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(arg for arg in args if arg.startswith('https://'))
root = pathlib.Path(os.environ['FIXTURE_ROOT'])
with (root / 'requests').open('a') as log:
    log.write(url + '\\n')
if url.endswith('/latest'):
    print('https://github.com/jeremielumandong/emulsion/releases/tag/v0.0.1', end='')
elif os.environ.get('FAIL_DOWNLOAD'):
    sys.exit(22)
else:
    shutil.copyfile(root / url.rsplit('/', 1)[-1], args[args.index('-o') + 1])
''')
        self.env = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}",
                        FIXTURE_ROOT=str(self.root), INSTALL_MARKER=str(self.marker))
        self.env.pop("EMULSION_VERSION", None)

    def command(self, name, source):
        path = self.bin / name
        path.write_text(source)
        path.chmod(0o755)

    def run_installer(self, *args, **env):
        return subprocess.run(["sh", "-s", "--", *args], input=INSTALLER.read_text(),
                              text=True, capture_output=True, env=dict(self.env, **env))

    def test_latest_installs_and_passes_options(self):
        result = self.run_installer("--force")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--force", self.marker.read_text())
        requests = (self.root / "requests").read_text().splitlines()
        self.assertEqual(len(requests), 3)
        self.assertTrue(all('/download/v0.0.1/' in url for url in requests[1:]))

    def test_pinned_release_skips_latest(self):
        result = self.run_installer(EMULSION_VERSION="v0.0.1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('/latest', (self.root / "requests").read_text())

    def test_corrupt_archive_never_runs_installer(self):
        with (self.root / ASSET).open('ab') as archive:
            archive.write(b'corrupt')
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Checksum mismatch', result.stderr)
        self.assertFalse(self.marker.exists())

    def test_download_failure_never_runs_installer(self):
        result = self.run_installer(FAIL_DOWNLOAD="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.marker.exists())

    def test_unsupported_architecture_never_downloads(self):
        result = self.run_installer(TEST_ARCH="aarch64")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "requests").exists())

    def test_invalid_tag_never_downloads(self):
        result = self.run_installer(EMULSION_VERSION="v1/../../main")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "requests").exists())


if __name__ == "__main__":
    unittest.main()
