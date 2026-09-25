#!/usr/bin/env python3
"""Exercise shared release uploads without GitHub or signing credentials."""
import hashlib
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('release_assets', Path(__file__).with_name('upload-release-assets.py'))
release_assets = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release_assets)

SHA = 'a' * 40
OTHER_SHA = 'b' * 40
REPOSITORY = 'owner/emulsion'
TAG = 'v0.0.2'


class FakeGitHub:
    def __init__(self):
        self.release = None
        self.reference = None
        self.tags = {}
        self.creates = []
        self.race = False

    def __call__(self, path, data=None, missing_ok=False):
        if data is not None:
            self.creates.append(data)
            self.release = {**data, 'html_url': 'https://github.com/owner/emulsion/releases/tag/v0.0.2'}
            if self.race:
                raise HTTPError(path, 422, 'Release already exists', {}, None)
            return self.release
        if '/releases?' in path:
            return [self.release] if self.release is not None else []
        if '/git/ref/tags/' in path:
            return self.reference
        if '/git/tags/' in path:
            return self.tags[path.rsplit('/', 1)[1]]
        raise AssertionError(f'Unexpected API path: {path}')


class ReleaseAssetsTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.api = FakeGitHub()
        sleeper = patch.object(release_assets.time, 'sleep')
        self.sleep = sleeper.start()
        self.addCleanup(sleeper.stop)

    def assets(self, platform):
        if platform == 'windows':
            asset = self.root / 'target/windows/emulsion_0.0.2_x64-setup.exe'
        else:
            asset = self.root / 'target/release-assets/Emulsion-linux-x86_64.tar.gz'
            appimage = self.root / 'target/appimage/Emulsion-0.0.2-x86_64.AppImage'
            appimage.parent.mkdir(parents=True, exist_ok=True)
            appimage.write_bytes(b'appimage fixture')
        asset.parent.mkdir(parents=True, exist_ok=True)
        asset.write_bytes(f'{platform} fixture'.encode())
        digest = hashlib.sha256(asset.read_bytes()).hexdigest()
        asset.with_name(asset.name + '.sha256').write_text(f'{digest}  {asset.name}\n')
        return asset

    def upload(self, platform):
        return release_assets.upload(self.root, platform, '0.0.2', REPOSITORY, SHA, self.api)

    def test_either_platform_can_create_the_shared_draft(self):
        self.assets('linux')
        self.assets('windows')
        for order in [('linux', 'windows'), ('windows', 'linux')]:
            with self.subTest(order=order), patch.object(release_assets.subprocess, 'run') as run:
                self.api = FakeGitHub()
                for platform in order:
                    self.upload(platform)
                self.assertEqual(len(self.api.creates), 1)
                self.assertTrue(self.api.release['draft'])
                self.assertEqual(self.api.release['target_commitish'], SHA)
                self.assertEqual(run.call_count, 2)
                for call in run.call_args_list:
                    command = call.args[0]
                    self.assertEqual(command[:4], ['gh', 'release', 'upload', TAG])
                    self.assertIn('--clobber', command)
                    self.assertTrue(call.kwargs['check'])
                first_files = set(run.call_args_list[0].args[0][7:])
                second_files = set(run.call_args_list[1].args[0][7:])
                self.assertFalse(first_files & second_files)

    def test_windows_alias_preserves_bytes_and_has_its_own_checksum_filename(self):
        original = self.assets('windows')
        assets = release_assets.release_assets(self.root, 'windows', '0.0.2')
        self.assertEqual(len(assets), 4)
        alias, checksum = assets[2:]
        self.assertEqual(alias.name, 'Emulsion-windows-x64-setup.exe')
        self.assertEqual(alias.read_bytes(), original.read_bytes())
        self.assertEqual(checksum.read_text().split(), [hashlib.sha256(alias.read_bytes()).hexdigest(), alias.name])

    def test_draft_retry_reuses_release(self):
        self.assets('windows')
        with patch.object(release_assets.subprocess, 'run') as run:
            self.upload('windows')
            self.upload('windows')
            self.assertEqual(len(self.api.creates), 1)
            self.assertEqual(run.call_count, 2)

    def test_simultaneous_creation_reuses_matching_draft(self):
        self.api.race = True
        result = release_assets.ensure_draft(self.api, REPOSITORY, TAG, SHA)
        self.assertTrue(result['draft'])
        self.assertEqual(result['target_commitish'], SHA)

    def test_new_draft_listing_delay_never_posts_a_second_draft(self):
        for conflict in (False, True):
            with self.subTest(conflict=conflict):
                self.api = FakeGitHub()
                self.api.race = conflict
                hidden_reads = 0

                def delayed(path, data=None, missing_ok=False):
                    nonlocal hidden_reads
                    if '/releases?' in path and self.api.release is not None:
                        hidden_reads += 1
                        if hidden_reads <= 2:
                            return []
                    return self.api(path, data=data, missing_ok=missing_ok)

                result = release_assets.ensure_draft(delayed, REPOSITORY, TAG, SHA)
                self.assertEqual(result['target_commitish'], SHA)
                self.assertEqual(len(self.api.creates), 1)
                self.assertEqual(hidden_reads, 3)

    def test_visibility_timeout_is_bounded_and_never_uploads(self):
        self.assets('linux')

        def invisible(path, data=None, missing_ok=False):
            if '/releases?' in path:
                return []
            return self.api(path, data=data, missing_ok=missing_ok)

        with patch.object(release_assets.subprocess, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'not yet visible.*do not delete'):
                release_assets.upload(self.root, 'linux', '0.0.2', REPOSITORY, SHA, invisible)
            self.assertEqual(len(self.api.creates), 1)
            self.assertEqual(self.sleep.call_count, 5)
            run.assert_not_called()

    def test_validation_error_keeps_github_details_when_no_draft_exists(self):
        def invalid(path, data=None, missing_ok=False):
            if data is not None:
                raise HTTPError(path, 422, 'Validation failed', {},
                                io.BytesIO(b'{"message":"Invalid tag name"}'))
            return self.api(path, missing_ok=missing_ok)

        with self.assertRaisesRegex(ValueError, 'HTTP 422.*Invalid tag name'):
            release_assets.ensure_draft(invalid, REPOSITORY, TAG, SHA)
        self.assertEqual(self.sleep.call_count, 5)

    def test_delayed_conflicting_draft_still_blocks_upload(self):
        self.assets('linux')
        hidden_reads = 0

        def conflicting(path, data=None, missing_ok=False):
            nonlocal hidden_reads
            if data is not None:
                self.api.release = {**data, 'target_commitish': OTHER_SHA}
                raise HTTPError(path, 422, 'Conflict', {}, None)
            if '/releases?' in path and self.api.release is not None:
                hidden_reads += 1
                if hidden_reads == 1:
                    return []
            return self.api(path, missing_ok=missing_ok)

        with patch.object(release_assets.subprocess, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'different commit'):
                release_assets.upload(self.root, 'linux', '0.0.2', REPOSITORY, SHA, conflicting)
            run.assert_not_called()

    def test_published_or_different_commit_release_never_uploads(self):
        self.assets('windows')
        for draft, commit in [(False, SHA), (True, OTHER_SHA), (True, 'main')]:
            with self.subTest(draft=draft, commit=commit), patch.object(release_assets.subprocess, 'run') as run:
                self.api.release = {'tag_name': TAG, 'draft': draft, 'target_commitish': commit}
                with self.assertRaises(ValueError):
                    self.upload('windows')
                run.assert_not_called()

    def test_existing_lightweight_and_annotated_tags_must_match_build(self):
        for commit in [SHA, OTHER_SHA]:
            for annotated in [False, True]:
                with self.subTest(commit=commit, annotated=annotated):
                    obj = {'type': 'commit', 'sha': commit}
                    self.api.reference = {'object': {'type': 'tag', 'sha': 'tag-object'} if annotated else obj}
                    self.api.tags['tag-object'] = {'object': obj}
                    if commit == SHA:
                        release_assets.check_release(self.api, REPOSITORY, TAG, SHA)
                    else:
                        with self.assertRaises(ValueError):
                            release_assets.check_release(self.api, REPOSITORY, TAG, SHA)

    def test_preflight_does_not_create_a_release(self):
        self.assertIsNone(release_assets.check_release(self.api, REPOSITORY, TAG, SHA))
        self.assertEqual(self.api.creates, [])

    def test_finds_drafts_beyond_the_first_page(self):
        wanted = {'tag_name': TAG, 'draft': True, 'target_commitish': SHA}
        def pages(path):
            if path.endswith('page=1'):
                return [{'tag_name': f'v1.0.{index}'} for index in range(100)]
            self.assertTrue(path.endswith('page=2'))
            return [wanted]
        self.assertEqual(release_assets.find_release(pages, REPOSITORY, TAG), wanted)

    def test_missing_assets_or_wrong_version_never_creates_release(self):
        self.assets('windows')
        with patch.object(release_assets.subprocess, 'run') as run:
            with self.assertRaises(ValueError):
                release_assets.upload(self.root, 'windows', '0.0.3', REPOSITORY, SHA, self.api)
            self.assertEqual(self.api.creates, [])
            run.assert_not_called()

    def test_modified_installer_fails_checksum_before_release_creation(self):
        installer = self.assets('windows')
        installer.write_bytes(b'changed after checksum')
        with patch.object(release_assets.subprocess, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'Checksum'):
                self.upload('windows')
            self.assertEqual(self.api.creates, [])
            run.assert_not_called()

    def test_failed_upload_fails_the_job(self):
        self.assets('windows')
        with patch.object(release_assets.subprocess, 'run', side_effect=subprocess.CalledProcessError(1, 'gh')):
            with self.assertRaises(subprocess.CalledProcessError):
                self.upload('windows')

    def test_creation_errors_are_not_treated_as_missing_releases(self):
        def forbidden(path, data=None, missing_ok=False):
            raise HTTPError(path, 403, 'Forbidden', {}, None)
        with self.assertRaises(HTTPError):
            release_assets.ensure_draft(forbidden, REPOSITORY, TAG, SHA)


if __name__ == '__main__':
    unittest.main()
