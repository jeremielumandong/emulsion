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
for main CI, then run both packaging workflows from the same main commit. Attach
the Windows installer to the Linux-created draft and publish when both are ready.

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
3. Run **Linux release package** from GitHub Actions on `main`.
   The workflow refuses to package a commit whose latest CI run has not passed.
4. The workflow builds on Ubuntu 24.04 and creates a **draft** release named
   `v<workspace version>`, with the installer archive, checksum, and standalone
   AppImage attached. Review and publish the draft when ready.

Use a new version for each release. Existing releases are not overwritten by the
workflow. Drafts and prereleases are not selected by the installer's default
`latest` URL. CI still runs before a release; creating an installer does not bypass
tests. The workflow does not deploy the website.

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
Only a verified setup and its SHA-256 checksum are uploaded as a workflow artifact
(retained for 14 days). Download that artifact from the run, extract it, and upload
the installer to Cloudflare R2 manually. This workflow does not create a GitHub
Release or publish to R2; the Linux release workflow remains separate.

The job reads signing configuration from the `windows-release` GitHub Environment.
Keep account identifiers, profile names, and publisher details in GitHub settings;
do not commit those values. Azure OIDC provides authentication without a client
secret. Configure the federated identity to match this repository and environment,
restrict the environment to `main`, and grant the CI identity signing access to
the intended certificate profile.

The federated credential must match the token's `sub` claim exactly. This
repository's tokens use GitHub's immutable-ID subject format, which includes the
numeric owner and repository IDs:

```text
Issuer:   https://token.actions.githubusercontent.com
Audience: api://AzureADTokenExchange
Subject:  repo:<owner>@<owner-id>/<repo>@<repo-id>:environment:windows-release
```

The Entra portal's "GitHub Actions deploying Azure resources" template builds the
older `repo:<owner>/<repo>:environment:windows-release` subject, which fails with
`AADSTS700213: No matching federated identity record found`. Use the **Other
issuer** scenario (or edit the subject) and paste the subject from that error
message verbatim.

Under **Settings → Actions → General**, add `azure/login@*` and
`actions/upload-artifact@*` to the existing action allowlist. All workflow actions
are pinned to commit SHAs. The Windows runner must provide .NET 8 and the Windows
SDK x64 signing tools; the signing helper downloads Microsoft's signing client.

For a local signed build, supply signing settings through the PowerShell
environment, authenticate with `az login`, and run
`./scripts/build-windows.ps1 -Sign`. Ordinary `-Package` builds remain unsigned.
