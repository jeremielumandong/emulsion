# Releases

## Prepare the version

When you decide to make a release, run **Actions → Prepare release → Run workflow**
on `main`. Choose `patch`, `minor`, or `major` (default: `patch`). For example,
patch changes `0.0.1` to `0.0.2`; minor changes it to `0.1.0`.

This is a manual workflow only. Ordinary commits, PR merges, and the Linux/Windows
package workflows do not increment the version. It opens a review PR updating the
workspace version in `Cargo.toml` and the matching package entries in `Cargo.lock`,
without upgrading dependencies or creating a release tag. It refuses to overwrite
an existing version preparation branch or tag.

Enable **Settings → Actions → General → Workflow permissions → Allow GitHub Actions
to create and approve pull requests**. The workflow only creates PRs; it does not
approve or merge them. GitHub may require you to approve CI on a bot-created PR.
After approving the run if requested, wait for CI and merge the version PR. Wait
for main CI, then run both packaging workflows from the same main commit. Both
upload their assets to the same draft automatically; publish after both succeed.

You can preview the next version locally without changing files:

```sh
python3 scripts/bump-version.py patch --dry-run
```

## Linux downloads

`curl -fsSL https://emulsion.pro/install | sh` downloads the latest published
GitHub Release from `jeremielumandong/emulsion`. The website serves the POSIX shell
bootstrap from `site/public/install`; deploy the updated website container before
advertising the command. Downloads require these two assets on the release:

- `Emulsion-linux-x86_64.tar.gz`
- `Emulsion-linux-x86_64.tar.gz.sha256`

The archive includes the AppImage, desktop entry, and the existing desktop
installer. License notices are inside the AppImage. The bootstrap resolves the
latest tag once so that the archive and checksum always come from the same release.
It verifies the archive before extracting or running it. Checksums detect corrupt
downloads; they are not an independent publisher signature.

## Build and publish

1. Run **Prepare release** and merge its version PR, as described above.
2. Wait for CI on that exact commit to pass.
3. Run **Linux release package** and **Windows signed release package** from
   GitHub Actions on `main`, against the same commit. Either can run first.
   Both workflows share a concurrency group: if started together, one waits for
   the other to finish. This prevents duplicate drafts during release creation.
   Both refuse to package a commit whose latest CI run has not passed.
4. The first workflow to finish creates a **draft** release named
   `v<workspace version>`. The other adds its assets to that draft. Linux attaches
   its installer archive, checksum, and standalone AppImage; Windows attaches
   its signed installer and checksums, including a stable download filename.
5. Wait for both workflows to succeed, then review and publish the shared draft
   as the latest release. Publishing before both complete would leave one
   platform's latest-download URL without its asset.

Use a new version for each published release. The workflows refuse to modify a
published release or reuse a draft/tag targeting a different commit. A rerun can
replace that platform's assets on a matching draft without removing the other
platform's assets. Drafts and prereleases are not selected by the installer's default
`latest` URL. CI still runs before a release; creating an installer does not bypass
tests. The workflow does not deploy the website.

If a platform fails while creating or uploading a draft, keep the draft with
successful uploads and rerun the failed job at the same commit. The uploader
retries briefly while newly created drafts become visible in GitHub's release
list. If older workflows created an empty duplicate draft, remove only that
empty duplicate before retrying; keep the draft containing the other platform's
assets. Do not publish until both sets of assets are present.

For local packaging:

```sh
bash scripts/package-release.sh             # compile and package
bash scripts/package-release.sh --no-build  # package an existing release binary
```

Assets are written to `target/release-assets/` (ignored by Git). Commit the
packaging scripts and manifests, not the generated binaries. Local packages inherit
the system library requirements of the machine where the binary was built;
prefer the CI build for publication. The initial CI package targets Linux x86_64
with Ubuntu 24.04-era system libraries and a working graphics driver.

Installing puts the AppImage in `~/Applications`, the launcher in `~/.local/bin`,
and desktop integration under `${XDG_DATA_HOME:-~/.local/share}`. Add `~/.local/bin`
to PATH if needed. Existing installer options can be passed with `sh -s --`, e.g.:

```sh
curl -fsSL https://emulsion.pro/install | sh -s -- --uninstall
```

This uninstall path downloads and verifies the release's installer first; it keeps
settings and projects. A source checkout can also run
`scripts/install-appimage.sh --uninstall` without downloading a release.

Omapak submissions pin their own source commit. Creating a release or committing
these files does not change an existing Omapak submission.

## Signed Windows installer

After merging to `main` and passing CI, run **Windows signed release package**.
It builds on Windows x64, authenticates using Azure OIDC, signs the application,
embedded NSIS uninstaller and setup, then checks publisher identity, timestamp
and signature validity. The bundled third-party DLL signatures are also checked.
The verified setup and its SHA-256 checksum are retained as a workflow artifact
for 14 days and automatically uploaded to the shared draft GitHub Release:

- `emulsion_<version>_x64-setup.exe` and its `.sha256` file
- `Emulsion-windows-x64-setup.exe` and its `.sha256` file

The stable filename is a byte-for-byte copy of the signed versioned installer;
its checksum file names the stable copy. The website and README use
`https://github.com/jeremielumandong/emulsion/releases/latest/download/Emulsion-windows-x64-setup.exe`.
Once the draft is published as latest, that URL follows it automatically. No R2
upload, VM version file, or website redeployment is needed for later releases.
GitHub's latest-release selection controls the download; this is not a directory
scan that sorts filenames by version. Publish stable releases in version order.

Both packaging workflows use `scripts/upload-release-assets.py`. They check the
draft/tag before compilation and again before uploading, and verify the package
checksum. Windows has `contents: write` permission for attaching release assets;
its existing environment approval, OIDC, and signing checks still apply.

The job reads signing configuration from the `windows-release` GitHub Environment.
Keep account identifiers, profile names, and publisher details in GitHub settings;
do not commit those values. Azure OIDC provides authentication without a client
secret. Configure the federated identity to match this repository and environment,
restrict the environment to `main`, and grant the CI identity signing access to
the intended certificate profile.

Under **Settings → Actions → General**, add `azure/login@*` and
`actions/upload-artifact@*` to the existing action allowlist. All workflow actions
are pinned to commit SHAs. The Windows runner must provide .NET 8 and the Windows
SDK x64 signing tools; the signing helper downloads Microsoft's signing client.

For a local signed build, copy `.env.example` to `.env` in the repository root
and fill in `AZURE_CODESIGNING_ENDPOINT`, `AZURE_CODESIGNING_ACCOUNT`,
`AZURE_CODESIGNING_PROFILE`, and `EMULSION_SIGN_EXPECTED_SUBJECT`. Authenticate
with `az login`, then run `./scripts/build-windows.ps1 -Sign`. The signed installer
is written to `target/windows/`. Local signing needs the x64 .NET 8 runtime,
Windows SDK signing tools, and cargo-packager listed above.

The signing helper reads `.env` and then `.env.local`, regardless of the current
directory. Existing PowerShell or GitHub environment values take precedence over
both files. Only `AZURE_*` and `EMULSION_SIGN*` settings are imported, into the
current process and its children. Values can be single- or double-quoted; they
are read literally without variable expansion or command execution. Both local
files are ignored by Git. Ordinary `-Package` builds remain unsigned.

To package an already compiled native `target/release/emulsion.exe`, add
`-SkipBuild` to `-Package` or `-Sign`. This uses the existing executable; rebuild
first when source changes need to be included. CI compiles before Azure login,
then runs `-Sign -SkipBuild` so a second compilation cannot consume the short
GitHub OIDC assertion lifetime before signing starts.
