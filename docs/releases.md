# Linux releases

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

1. Update the workspace version in `Cargo.toml` and its entries in `Cargo.lock`
   for a new release, then commit and push to `main`.
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

Under **Settings → Actions → General**, add `azure/login@*` and
`actions/upload-artifact@*` to the existing action allowlist. All workflow actions
are pinned to commit SHAs. The Windows runner must provide .NET 8 and the Windows
SDK x64 signing tools; the signing helper downloads Microsoft's signing client.

For a local signed build, supply signing settings through the PowerShell
environment, authenticate with `az login`, and run
`./scripts/build-windows.ps1 -Sign`. Ordinary `-Package` builds remain unsigned.
