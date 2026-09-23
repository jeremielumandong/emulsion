# GPUI sources used by Emulsion

The 29 GPUI-family packages used by this app are editable local dependencies.
`gpui-kit`, components, and assets are pinned to 0.6.4; the GPUI platform family
is pinned to 0.3.5 (the separately versioned reqwest fork is 0.12.15).
Root `Cargo.toml` overrides the registry packages through `[patch.crates-io]`.
Other dependencies remain normal Cargo dependencies: this is not a fully offline
vendor directory for the entire application.

Each directory was extracted from its exact crates.io `.crate` archive after
verifying the archive SHA-256 against the original `Cargo.lock`. `UPSTREAM.json`
records those checksums, package metadata, VCS revisions where provided, and
original license-file hashes. `Cargo.toml.orig` and embedded upstream sources
are retained. No GPUI API version upgrade accompanies this import.

## Local patches

- [Core frame presentation](gpui-pre/EMULSION_CHANGES.md): skip unchanged-scene
  keepalive presentation on software devices; preserve virtual-list height
  estimates across layout and resizing.
- [Linux/wgpu](gpui-pre-wgpu/EMULSION_CHANGES.md): hardware-first ordering,
  forced software diagnostics, and software-capable device-loss recovery.
- [Windows](gpui-pre-windows/EMULSION_CHANGES.md): explicit WARP fallback and
  economical software frame pacing adapted from AgentOps' Apache-2.0 GPUI fork.
- `gpui-pre-reqwest/.gitignore`: allow its published `Cargo.lock` to be tracked
  with the rest of the archive; no reqwest code changes.

Native macOS Metal rendering is unchanged. These changes do not add GPU image
compositing or painting; those remain Emulsion's existing CPU implementations.

## Updating or editing

Keep patches small and keep the matching GPUI family in sync. Do not replace
these directories with arbitrary files from the latest Zed checkout. For an
upgrade, import checksum-verified archives, review license and asset changes,
update the provenance inventory and root patches, and deliberately regenerate
the lockfile. Preserve all copyright/license headers and mark modified files.
See [licensing](LICENSING.md); original Emulsion code is MIT, but upstream GPUI
files and our GPUI patches retain their stated upstream terms.

Run `python3 scripts/check-gpui-vendor.py`, `python3 scripts/test-license-staging.py`,
`bash scripts/test-renderer-policy.sh`, and the workspace Cargo checks from the
repository root. Native platform CI checks include macOS and Windows. The Linux
CI renderer smoke test uses a real X11 window and Mesa's CPU Vulkan driver.

Licenses, supplemental notices, provenance, and patch descriptions are staged
by the packaging scripts using `packaging/license-files.txt`. Include newly
added notices in that manifest; the staging regression check rejects omissions.
