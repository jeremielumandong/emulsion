#!/usr/bin/env bash
#
# build-appimage.sh — build Emulsion as an x86_64 AppImage.
#
# Output: target/appimage/Emulsion-<version>-x86_64.AppImage
#
# The binary links only against libc, libgcc, libm, xcb and xkbcommon; Vulkan,
# Wayland and fontconfig are loaded at runtime from the system, which is where
# they must come from anyway (the GPU driver owns Vulkan). So the AppDir holds
# the binary, the desktop entry, icons and license notices, without bundling
# these system libraries.
#
# appimagetool and the AppImage runtime are downloaded once into the cache,
# pinned by version and verified by sha256. Nothing else is fetched.
#
# Usage:
#   scripts/build-appimage.sh            # release build, then package
#   scripts/build-appimage.sh --no-build # package the existing release binary
#
# Env:
#   EMULSION_TOOLS_DIR   tool cache (default ~/.cache/emulsion/tools)
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS_DIR="${EMULSION_TOOLS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/emulsion/tools}"
OUT_DIR="$ROOT_DIR/target/appimage"
APPDIR="$OUT_DIR/Emulsion.AppDir"
ARCH=x86_64
APP_ID=app.emulsion.Emulsion

APPIMAGETOOL_VERSION=1.9.1
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

DO_BUILD=1
for arg in "$@"; do
  case "$arg" in
    --no-build) DO_BUILD=0 ;;
    -h|--help) awk '/^set -euo pipefail$/ { exit } NR > 1 { sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "build-appimage: unknown option '$arg' (try --help)" >&2; exit 2 ;;
  esac
done

log() { printf '\033[36m==>\033[0m %s\n' "$*"; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ "$(uname -m)" == "$ARCH" ]] || die "only $ARCH builds are supported (this machine is $(uname -m))"

# Download $2 to $1 unless present, and verify the sha256 either way.
fetch() {
  local dest="$1" url="$2" sha="$3"
  if [[ ! -f "$dest" ]]; then
    log "Downloading $(basename "$dest")"
    mkdir -p "$(dirname "$dest")"
    curl -fL --retry 3 -o "$dest.part" "$url"
    mv -f "$dest.part" "$dest"
  fi
  if [[ "$(sha256sum "$dest" | cut -d' ' -f1)" != "$sha" ]]; then
    rm -f "$dest"
    die "checksum mismatch for $(basename "$dest"); the download was removed, run again"
  fi
  chmod 755 "$dest"
}

VERSION=$(awk -F'"' '/^\[workspace.package\]/ { f = 1 } f && /^version/ { print $2; exit }' "$ROOT_DIR/Cargo.toml")
[[ -n "$VERSION" ]] || die "could not read the workspace version from Cargo.toml"

if (( DO_BUILD )); then
  log "Building emulsion $VERSION (release)"
  ( cd "$ROOT_DIR" && cargo build --release --locked -p emulsion-app )
fi
BIN="$ROOT_DIR/target/release/emulsion"
[[ -x "$BIN" ]] || die "$BIN is missing — run without --no-build"

APPIMAGETOOL="$TOOLS_DIR/appimagetool-$APPIMAGETOOL_VERSION-$ARCH.AppImage"
RUNTIME="$TOOLS_DIR/runtime-$RUNTIME_VERSION-$ARCH"
fetch "$APPIMAGETOOL" \
  "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$ARCH.AppImage" \
  "$APPIMAGETOOL_SHA256"
fetch "$RUNTIME" \
  "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-$ARCH" \
  "$RUNTIME_SHA256"

log "Assembling the AppDir"
rm -rf "$APPDIR"
install -Dm755 "$BIN" "$APPDIR/usr/bin/emulsion"
bash "$ROOT_DIR/scripts/stage-licenses.sh" "$APPDIR/usr/share/licenses/emulsion"
strip --strip-debug "$APPDIR/usr/bin/emulsion" 2>/dev/null || true
install -Dm644 "$ROOT_DIR/packaging/linux/$APP_ID.desktop" "$APPDIR/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$ROOT_DIR/packaging/linux/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
install -Dm644 "$ROOT_DIR/packaging/linux/$APP_ID.metainfo.xml" "$APPDIR/usr/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "$ROOT_DIR/assets/icons/emulsion.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/emulsion.svg"

render_png() {
  local size="$1" dest="$2"
  if command -v rsvg-convert >/dev/null; then
    rsvg-convert -w "$size" -h "$size" -o "$dest" "$ROOT_DIR/assets/icons/emulsion.svg"
  elif command -v magick >/dev/null; then
    magick -background none -density 384 "$ROOT_DIR/assets/icons/emulsion.svg" -resize "${size}x${size}" "$dest"
  else
    return 1
  fi
}
for size in 32 64 128 256 512; do
  mkdir -p "$APPDIR/usr/share/icons/hicolor/${size}x${size}/apps"
  render_png "$size" "$APPDIR/usr/share/icons/hicolor/${size}x${size}/apps/emulsion.png" \
    || die "need rsvg-convert or ImageMagick to render the icon"
done
cp "$APPDIR/usr/share/icons/hicolor/256x256/apps/emulsion.png" "$APPDIR/emulsion.png"
ln -sf emulsion.png "$APPDIR/.DirIcon"

cat >"$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
# Emulsion AppImage entry point.
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/emulsion" "$@"
APPRUN
chmod 755 "$APPDIR/AppRun"

OUT="$OUT_DIR/Emulsion-$VERSION-$ARCH.AppImage"
log "Packaging $(basename "$OUT")"
# Extract-and-run: building needs no FUSE. The runtime is passed explicitly so
# appimagetool does not download an unpinned one.
ARCH=$ARCH APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" --no-appstream \
  --runtime-file "$RUNTIME" "$APPDIR" "$OUT.part" >/dev/null
mv -f "$OUT.part" "$OUT"
chmod 755 "$OUT"
log "Built $OUT ($(du -h "$OUT" | cut -f1))"
