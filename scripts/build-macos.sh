#!/usr/bin/env bash
# Build and package Emulsion for the current macOS architecture.
#
# Output: target/macos/Emulsion.app
#         target/macos/Emulsion-<version>-<arch>.dmg
#
# Usage:
#   scripts/build-macos.sh            # release build, then package
#   scripts/build-macos.sh --no-build # package the existing release binary
#
# Requires iconutil and either rsvg-convert or ImageMagick for the app icon.
# The bundle is signed ad hoc for local use; distribution requires a Developer ID
# signature and notarization.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT_DIR/target/macos"
ARCH="$(uname -m)"

log() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

DO_BUILD=1
for arg in "$@"; do
  case "$arg" in
    --no-build) DO_BUILD=0 ;;
    -h|--help) sed -n '2,/^set -euo pipefail/{ /^set -euo pipefail/!s/^# \{0,1\}//p; }' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) die "unknown option '$arg' (try --help)" ;;
  esac
done

[[ "$(uname -s)" == Darwin ]] || die "this script must run on macOS"
case "$ARCH" in arm64|x86_64) ;; *) die "unsupported architecture: $ARCH" ;; esac
command -v iconutil >/dev/null || die "iconutil is required"
command -v hdiutil >/dev/null || die "hdiutil is required"
command -v codesign >/dev/null || die "codesign is required"
if ! command -v rsvg-convert >/dev/null && ! command -v magick >/dev/null; then
  die "install librsvg (rsvg-convert) or ImageMagick (magick) to render assets/icons/emulsion.svg"
fi

VERSION=$(awk -F'"' '/^\[workspace.package\]/ { f = 1 } f && /^version/ { print $2; exit }' "$ROOT_DIR/Cargo.toml")
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "expected a numeric workspace version in Cargo.toml"

if (( DO_BUILD )); then
  log "Building emulsion $VERSION (release, $ARCH)"
  ( cd "$ROOT_DIR" && cargo build --release --locked -p emulsion-app )
fi

BIN="$ROOT_DIR/target/release/emulsion"
[[ -x "$BIN" ]] || die "$BIN is missing; run without --no-build"
[[ "$(lipo -archs "$BIN")" == "$ARCH" ]] || die "$BIN is not a $ARCH binary"

mkdir -p "$OUT_DIR"
APP="$OUT_DIR/Emulsion.app"
DMG="$OUT_DIR/Emulsion-$VERSION-$ARCH.dmg"
STAGE="$OUT_DIR/.Emulsion-$VERSION-$ARCH-$$.app"
ICONSET="$OUT_DIR/.Emulsion-$$.iconset"
DMG_TEMP="$OUT_DIR/.Emulsion-$VERSION-$ARCH-$$.dmg"
trap 'rm -rf "$STAGE" "$ICONSET"; rm -f "$DMG_TEMP"' EXIT

log "Assembling $(basename "$APP")"
mkdir -p "$STAGE/Contents/MacOS" "$STAGE/Contents/Resources" "$ICONSET"
install -m 755 "$BIN" "$STAGE/Contents/MacOS/emulsion"
bash "$ROOT_DIR/scripts/stage-licenses.sh" "$STAGE/Contents/Resources/licenses"

render_icon() {
  local size="$1" dest="$2"
  if command -v rsvg-convert >/dev/null; then
    rsvg-convert -w "$size" -h "$size" -o "$dest" "$ROOT_DIR/assets/icons/emulsion.svg"
  else
    magick -background none "$ROOT_DIR/assets/icons/emulsion.svg" -resize "${size}x${size}" "$dest"
  fi
}

for size in 16 32 64 128 256 512; do
  render_icon "$size" "$ICONSET/icon_${size}x${size}.png"
done
for size in 16 32 64 128 256 512; do
  render_icon "$(( size * 2 ))" "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$STAGE/Contents/Resources/Emulsion.icns"

cat >"$STAGE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>emulsion</string>
  <key>CFBundleIconFile</key><string>Emulsion</string>
  <key>CFBundleIdentifier</key><string>app.emulsion.Emulsion</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Emulsion</string>
  <key>CFBundleDisplayName</key><string>Emulsion</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

plutil -lint "$STAGE/Contents/Info.plist" >/dev/null
codesign --force --sign - "$STAGE"
codesign --verify --strict "$STAGE"

rm -rf "$APP"
mv "$STAGE" "$APP"

log "Creating $(basename "$DMG")"
# Automatic sizing can leave too little space inside the temporary volume.
# Reserve filesystem/copy overhead explicitly; unused space compresses away.
APP_KIB=$(du -sk "$APP" | awk '{print $1}')
DMG_KIB=$(( APP_KIB + APP_KIB / 4 + 65536 ))
hdiutil create -format UDZO -fs HFS+ -size "${DMG_KIB}k" \
  -srcfolder "$APP" -volname "Emulsion" "$DMG_TEMP"
mv -f "$DMG_TEMP" "$DMG"
log "Built $APP"
log "Built $DMG"
