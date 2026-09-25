#!/usr/bin/env bash
# Build the downloadable Linux installer bundle and checksum in target/release-assets.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bash "$ROOT_DIR/scripts/build-appimage.sh" "$@"
VERSION=$(awk -F'"' '/^\[workspace.package\]/ { f = 1 } f && /^version/ { print $2; exit }' "$ROOT_DIR/Cargo.toml")
OUT="$ROOT_DIR/target/release-assets"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$OUT" "$STAGE/scripts" "$STAGE/packaging/linux"
cp "$ROOT_DIR/target/appimage/Emulsion-$VERSION-x86_64.AppImage" "$STAGE/Emulsion.AppImage"
cp "$ROOT_DIR/scripts/install-appimage.sh" "$STAGE/scripts/"
cp "$ROOT_DIR/packaging/linux/app.emulsion.Emulsion.desktop" "$STAGE/packaging/linux/"
tar -czf "$OUT/Emulsion-linux-x86_64.tar.gz" -C "$STAGE" Emulsion.AppImage scripts packaging
cd "$OUT"
sha256sum Emulsion-linux-x86_64.tar.gz > Emulsion-linux-x86_64.tar.gz.sha256
printf 'Release assets: %s\n' "$OUT"
