# Releases

## Prepare the version

When you decide to make a release, run **Actions → Prepare release → Run workflow**
on `main`. Choose `patch`, `minor`, or `major` (default: `patch`). For example,
patch changes `0.0.1` to `0.0.2`; minor changes it to `0.1.0`.

This is a manual workflow only. Ordinary commits, PR merges, and the package
workflows do not increment the version. It opens a review PR updating the
workspace version in `Cargo.toml` and the matching package entries in `Cargo.lock`,
without upgrading dependencies or creating a release tag. It refuses to overwrite
an existing version preparation branch or tag.

Enable **Settings → Actions → General → Workflow permissions → Allow GitHub Actions
to create and approve pull requests**. The workflow only creates PRs; it does not
approve or merge them. GitHub may require you to approve CI on a bot-created PR.
After approving the run if requested, wait for CI and merge the version PR. Wait
for main CI, then run all three packaging workflows from the same main commit. They
upload their assets to the same draft automatically; publish after all succeed.

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
3. Run **Linux release package**, **Windows signed release package**, and
   **macOS signed release package** from GitHub Actions on `main`, against the
   same commit. Any can run first. The workflows share a concurrency group: if
   started together, they run one after another. This prevents duplicate drafts
   during release creation. All refuse to package a commit whose latest CI run
   has not passed.
4. The first workflow to finish creates a **draft** release named
   `v<workspace version>`. The others add their assets to that draft. Linux attaches
   its installer archive, checksum, and standalone AppImage; Windows attaches
   its signed installer and checksums, including a stable download filename;
   macOS attaches notarized Apple silicon and Intel disk images the same way.
5. Wait for all three workflows to succeed, then review and publish the shared
   draft as the latest release. Publishing before all complete would leave a
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
assets. Do not publish until every platform's assets are present.

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

All packaging workflows use `scripts/upload-release-assets.py`. They check the
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

## Signed macOS disk images

After merging to `main` and passing CI, run **macOS signed release package**. It
builds on two hosted runners in parallel, `macos-15` (Apple silicon, `arm64`) and
`macos-15-intel` (`x86_64`). Each compiles the release executable before any
credentials are loaded, then `scripts/build-macos.sh --no-build --sign` signs the
app with the Developer ID certificate, hardened runtime and a secure timestamp,
signs the disk image, notarizes it with `notarytool`, staples the ticket and
checks it with Gatekeeper (`spctl`). The build refuses bundles that link non-system
libraries. A final job attaches both images to the shared draft, one at a time:

- `Emulsion-<version>-arm64.dmg` and `Emulsion-<version>-x86_64.dmg`, with `.sha256` files
- `Emulsion-macos-arm64.dmg` and `Emulsion-macos-x86_64.dmg`, with `.sha256` files

As on Windows, the stable filenames are byte-for-byte copies for
`releases/latest/download/` links. The notarization ticket is stapled inside the
image, so renaming does not affect it. Images are also kept as workflow artifacts
for 14 days. If one architecture fails, rerun the failed jobs; the successful
architecture's artifact is reused.

### One-time setup

1. In the Apple Developer account (Account Holder role), create a **Developer ID
   Application** certificate. Export it with its private key from Keychain Access
   as a password-protected `.p12`.
2. In App Store Connect → Users and Access → Integrations → **Team Keys**, create an
   API key with the Developer role. Download the `.p8` (only possible once) and
   note its Key ID and Issuer ID.
3. Create a GitHub Environment named `macos-release`, restricted to `main`, with:

   | Kind | Name | Value |
   |---|---|---|
   | Variable | `MACOS_SIGN_IDENTITY` | `Developer ID Application: <name> (<team id>)`, without quotes |
   | Variable | `APPLE_TEAM_ID` | the 10-character team ID |
   | Secret | `MACOS_CERT_P12_BASE64` | output of `base64 -i developer-id.p12` |
   | Secret | `MACOS_CERT_PASSWORD` | the `.p12` export password |
   | Secret | `APPLE_API_KEY_P8_BASE64` | output of `base64 -i AuthKey_<id>.p8` |
   | Secret | `APPLE_API_KEY_ID` | the API key ID |
   | Secret | `APPLE_API_ISSUER_ID` | the issuer ID |

4. Add `actions/download-artifact@*` to the action allowlist under **Settings →
   Actions → General**, next to `actions/upload-artifact@*`.

The certificate is imported into a temporary keychain that is deleted at the
end of the job. Delete the local `.p12`, `.p8`, and base64 copies once they are
stored in GitHub. The Developer ID certificate is valid for five years; renew it
and replace `MACOS_CERT_P12_BASE64` before it expires.

For a local signed build on a Mac with the certificate in the login keychain:

```sh
export MACOS_SIGN_IDENTITY="Developer ID Application: <name> (<team id>)"
export APPLE_TEAM_ID=<team id>
export APPLE_API_KEY_PATH=~/keys/AuthKey_<id>.p8 APPLE_API_KEY_ID=<id> APPLE_API_ISSUER_ID=<issuer>
scripts/build-macos.sh --sign
```

Without `--sign`, local builds stay ad hoc signed and are not notarized.
