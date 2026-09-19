#!/usr/bin/env bash
# build-flatpak.sh — build Emulsion as a Flatpak and install it for the
# current user.
#
# Needs: flatpak, flatpak-builder, and the freedesktop 24.08 runtime with the
# rust-stable SDK extension (installed on first run from Flathub).
# Optional: flatpak-cargo-generator (pip install flatpak-cargo-generator or
# the flatpak-builder-tools checkout) to write cargo-sources.json for an
# offline build; without it the build fetches crates over the network.
#
# Usage:
#   scripts/build-flatpak.sh              # build + install --user
#   scripts/build-flatpak.sh --run        # also launch it afterwards
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT_DIR/packaging/flatpak/app.emulsion.Emulsion.yml"
BUILD_DIR="$ROOT_DIR/target/flatpak/build"
STATE_DIR="$ROOT_DIR/target/flatpak/state"
RUN_AFTER=0
for arg in "$@"; do
  case "$arg" in
    --run) RUN_AFTER=1 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "build-flatpak: unknown option '$arg' (try --help)" >&2; exit 2 ;;
  esac
done

for tool in flatpak flatpak-builder; do
  command -v "$tool" >/dev/null || { echo "build-flatpak: $tool is not installed" >&2; exit 1; }
done

flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user -y --noninteractive flathub \
  org.freedesktop.Platform//24.08 org.freedesktop.Sdk//24.08 \
  org.freedesktop.Sdk.Extension.rust-stable//24.08

if command -v flatpak-cargo-generator >/dev/null; then
  echo "==> Writing cargo-sources.json from Cargo.lock"
  flatpak-cargo-generator "$ROOT_DIR/Cargo.lock" -o "$ROOT_DIR/packaging/flatpak/cargo-sources.json"
fi

mkdir -p "$BUILD_DIR" "$STATE_DIR"
echo "==> flatpak-builder"
flatpak-builder --user --install --force-clean --ccache \
  --state-dir "$STATE_DIR" "$BUILD_DIR" "$MANIFEST"
echo "==> Installed app.emulsion.Emulsion for this user."
if [ "$RUN_AFTER" = 1 ]; then
  flatpak run app.emulsion.Emulsion
fi
