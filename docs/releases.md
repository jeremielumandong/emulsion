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
