#!/usr/bin/env python3
"""Create or extend a shared draft release for the current workspace version."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import tomllib
from urllib.error import HTTPError
from urllib.request import Request, urlopen


def github_api(path, data=None, missing_ok=False):
    request = Request(
        os.environ.get('GITHUB_API_URL', 'https://api.github.com') + path,
        data=json.dumps(data).encode() if data is not None else None,
        headers={
            'Authorization': f'Bearer {os.environ["GH_TOKEN"]}',
            'Accept': 'application/vnd.github+json',
            'Content-Type': 'application/json',
            'User-Agent': 'emulsion-release',
        },
    )
    try:
        with urlopen(request, timeout=60) as response:
            return json.load(response)
    except HTTPError as error:
        if missing_ok and error.code == 404:
            return None
        raise


def find_release(api, repository, tag):
    # The tag endpoint is for published releases. Listing with push access also
    # includes drafts, even when the release tag has not been created yet.
    page = 1
    while True:
        releases = api(f'/repos/{repository}/releases?per_page=100&page={page}')
        for release in releases:
            if release['tag_name'] == tag:
                return release
        if len(releases) < 100:
            return None
        page += 1


def check_release(api, repository, tag, sha):
    base = f'/repos/{repository}'
    release = find_release(api, repository, tag)
    if release is not None:
        if not release['draft']:
            raise ValueError(f'{tag} is already published. Prepare a new version instead.')
        if release['target_commitish'] != sha:
            raise ValueError(f'{tag} targets a different commit. Both platforms must build the same commit.')
    # An existing tag takes precedence over target_commitish when GitHub publishes.
    reference = api(f'{base}/git/ref/tags/{tag}', missing_ok=True)
    if reference is not None:
        obj = reference['object']
        seen = set()
        while obj['type'] == 'tag':
            if obj['sha'] in seen:
                raise ValueError('Cyclic annotated release tag.')
            seen.add(obj['sha'])
            obj = api(f'{base}/git/tags/{obj["sha"]}')['object']
        if obj['type'] != 'commit' or obj['sha'] != sha:
            raise ValueError(f'Tag {tag} points to a different commit.')
    return release


def ensure_draft(api, repository, tag, sha):
    release = check_release(api, repository, tag, sha)
    if release is None:
        creation_error = None
        try:
            api(f'/repos/{repository}/releases', data={
                'tag_name': tag,
                'target_commitish': sha,
                'name': f'Emulsion {tag}',
                'draft': True,
                'generate_release_notes': True,
            })
        except HTTPError as error:
            if error.code != 422:
                raise
            creation_error = error
        # The release list can briefly lag behind a successful POST (or a
        # conflicting creator). Retry reads only; another POST can create a
        # duplicate draft. Workflows share a concurrency group to avoid races.
        for delay in (0, 1, 2, 4, 8, 16):
            if delay:
                time.sleep(delay)
            release = check_release(api, repository, tag, sha)
            if release is not None:
                break
        if release is None:
            if creation_error is not None:
                details = creation_error.read().decode('utf-8', errors='replace')
                raise ValueError(f'GitHub rejected draft {tag} (HTTP 422): {details}') from creation_error
            raise ValueError(f'Draft {tag} was created but is not yet visible. Retry this workflow; do not delete the draft.')
    return release


MACOS_ARCHES = {'macos-arm64': 'arm64', 'macos-x86_64': 'x86_64'}
PLATFORMS = ['linux', 'windows', *MACOS_ARCHES]


def release_assets(root, platform, version):
    if platform == 'linux':
        assets = [
            root / 'target/release-assets/Emulsion-linux-x86_64.tar.gz',
            root / 'target/release-assets/Emulsion-linux-x86_64.tar.gz.sha256',
            root / f'target/appimage/Emulsion-{version}-x86_64.AppImage',
        ]
    elif platform == 'windows':
        installer = root / f'target/windows/emulsion_{version}_x64-setup.exe'
        assets = [installer, installer.with_name(installer.name + '.sha256')]
        stable_name = 'Emulsion-windows-x64-setup.exe'
    else:
        arch = MACOS_ARCHES[platform]
        installer = root / f'target/macos/Emulsion-{version}-{arch}.dmg'
        assets = [installer, installer.with_name(installer.name + '.sha256')]
        stable_name = f'Emulsion-macos-{arch}.dmg'
    for asset in assets:
        if not asset.is_file() or asset.stat().st_size == 0:
            raise ValueError(f'Missing or empty release asset: {asset}')

    with assets[0].open('rb') as source:
        digest = hashlib.file_digest(source, 'sha256').hexdigest()
    checksum = assets[1].read_text(encoding='utf-8-sig').strip().split()
    if checksum != [digest, assets[0].name]:
        raise ValueError(f'Checksum does not match {assets[0].name}.')

    if platform != 'linux':
        # Renaming a signed installer preserves its bytes and signature; the
        # notarization ticket is stapled inside the disk image, not its name.
        alias = root / 'target/release-assets' / stable_name
        alias.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(assets[0], alias)
        alias_checksum = alias.with_name(alias.name + '.sha256')
        alias_checksum.write_text(f'{digest}  {alias.name}\n', encoding='ascii')
        assets.extend([alias, alias_checksum])
    return assets


def upload(root, platform, version, repository, sha, api=github_api):
    assets = release_assets(root, platform, version)
    tag = f'v{version}'
    release = ensure_draft(api, repository, tag, sha)
    subprocess.run([
        'gh', 'release', 'upload', tag, '--repo', repository, '--clobber',
        *map(str, assets),
    ], check=True)
    return release


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('platform', choices=PLATFORMS)
    parser.add_argument('--check', action='store_true', help='Check the release before compiling; do not upload.')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    with (root / 'Cargo.toml').open('rb') as source:
        version = tomllib.load(source)['workspace']['package']['version']
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
        raise ValueError('Release version must be stable major.minor.patch.')
    repository = os.environ['GITHUB_REPOSITORY']
    sha = os.environ['GITHUB_SHA']
    if not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('GITHUB_SHA must identify an exact commit.')
    if args.check:
        check_release(github_api, repository, f'v{version}', sha)
        print(f'Release v{version} is ready for {args.platform} packaging at {sha}.')
    else:
        release = upload(root, args.platform, version, repository, sha)
        message = (f'{args.platform.capitalize()} assets uploaded to draft v{version}: {release["html_url"]}\n'
                   'Publish the draft only after Linux, Windows, and both macOS assets are attached.\n')
        print(message)
        if os.environ.get('GITHUB_STEP_SUMMARY'):
            with open(os.environ['GITHUB_STEP_SUMMARY'], 'a', encoding='utf-8') as summary:
                summary.write(message)


if __name__ == '__main__':
    main()
