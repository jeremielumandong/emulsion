#!/usr/bin/env bash
# Build and package Emulsion for the current macOS architecture.
#
# Output: target/macos/Emulsion.app
#         target/macos/Emulsion-<version>-<arch>.dmg
#
# Usage:
#   scripts/build-macos.sh            # release build, then package
#   scripts/build-macos.sh --no-build # package the existing release binary
#   scripts/build-macos.sh --sign     # Developer ID signature and notarization
#
# Requires the standard macOS iconutil and sips tools for the app icon.
# Without --sign the bundle is signed ad hoc for local use. --sign reads:
#   MACOS_SIGN_IDENTITY   "Developer ID Application: <name> (<team id>)"
#   APPLE_TEAM_ID         expected team identifier of that certificate
#   APPLE_API_KEY_PATH    App Store Connect API key (.p8) for notarytool
#   APPLE_API_KEY_ID      its key ID
#   APPLE_API_ISSUER_ID   its issuer ID

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT_DIR/target/macos"
ARCH="$(uname -m)"

log() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

DO_BUILD=1
DO_SIGN=0
for arg in "$@"; do
  case "$arg" in
    --no-build) DO_BUILD=0 ;;
    --sign) DO_SIGN=1 ;;
    -h|--help) sed -n '2,/^set -euo pipefail/{ /^set -euo pipefail/!s/^# \{0,1\}//p; }' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) die "unknown option '$arg' (try --help)" ;;
  esac
done

[[ "$(uname -s)" == Darwin ]] || die "this script must run on macOS"
case "$ARCH" in arm64|x86_64) ;; *) die "unsupported architecture: $ARCH" ;; esac
command -v iconutil >/dev/null || die "iconutil is required"
command -v hdiutil >/dev/null || die "hdiutil is required"
command -v codesign >/dev/null || die "codesign is required"
command -v sips >/dev/null || die "sips is required"

if (( DO_SIGN )); then
  for name in MACOS_SIGN_IDENTITY APPLE_TEAM_ID APPLE_API_KEY_PATH APPLE_API_KEY_ID APPLE_API_ISSUER_ID; do
    [[ -n "${!name:-}" ]] || die "--sign requires $name"
  done
  [[ "$MACOS_SIGN_IDENTITY" == "Developer ID Application: "*" ($APPLE_TEAM_ID)" ]] \
    || die "MACOS_SIGN_IDENTITY must be a Developer ID Application identity for team $APPLE_TEAM_ID"
  [[ -s "$APPLE_API_KEY_PATH" ]] || die "APPLE_API_KEY_PATH does not name a key file"
  security find-identity -v -p codesigning | grep -Fq "\"$MACOS_SIGN_IDENTITY\"" \
    || die "signing identity is not in the keychain search list: $MACOS_SIGN_IDENTITY"
  xcrun --find notarytool >/dev/null || die "notarytool is required"
  xcrun --find stapler >/dev/null || die "stapler is required"
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
# The bundle ships only the executable: any other non-system library would be
# missing on users' Macs and would be rejected by the hardened runtime.
if otool -L "$BIN" | tail -n +2 | awk '{print $1}' | grep -Ev '^(/System/Library/|/usr/lib/)'; then
  die "$BIN links libraries that are not part of macOS (listed above)"
fi

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
  sips -z "$size" "$size" "$ROOT_DIR/assets/icons/emulsion.png" --out "$dest" >/dev/null
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
if (( DO_SIGN )); then
  log "Signing with $MACOS_SIGN_IDENTITY"
  # Notarization requires the hardened runtime and a secure timestamp.
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$STAGE"
  codesign --verify --strict --deep "$STAGE"
  details=$(codesign -dvv "$STAGE" 2>&1)
  grep -Fxq "Authority=$MACOS_SIGN_IDENTITY" <<<"$details" || die "unexpected signing authority"
  grep -Fxq "TeamIdentifier=$APPLE_TEAM_ID" <<<"$details" || die "unexpected team identifier"
  grep -q '^CodeDirectory .*flags=.*(runtime)' <<<"$details" || die "hardened runtime is not enabled"
else
  codesign --force --sign - "$STAGE"
  codesign --verify --strict "$STAGE"
fi

rm -rf "$APP"
mv "$STAGE" "$APP"

log "Creating $(basename "$DMG")"
# Automatic sizing can leave too little space inside the temporary volume.
# Reserve filesystem/copy overhead explicitly; unused space compresses away.
APP_KIB=$(du -sk "$APP" | awk '{print $1}')
DMG_KIB=$(( APP_KIB + APP_KIB / 4 + 65536 ))
# Hosted runners occasionally report "Resource busy" while hdiutil scans the
# source folder; a short retry avoids failing a long release build.
for attempt in 1 2 3; do
  rm -f "$DMG_TEMP"
  hdiutil create -format UDZO -fs HFS+ -size "${DMG_KIB}k" \
    -srcfolder "$APP" -volname "Emulsion" "$DMG_TEMP" && break
  (( attempt < 3 )) || die "hdiutil could not create the disk image"
  sleep $(( attempt * 10 ))
done

if (( DO_SIGN )); then
  codesign --force --timestamp --sign "$MACOS_SIGN_IDENTITY" "$DMG_TEMP"
  codesign --verify --strict "$DMG_TEMP"

  log "Notarizing $(basename "$DMG") (usually a few minutes)"
  NOTARY=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID")
  result=$(xcrun notarytool submit "$DMG_TEMP" "${NOTARY[@]}" --wait --timeout 60m --output-format json) || true
  [[ -n "$result" ]] || die "notarytool could not submit $(basename "$DMG")"
  submission=$(plutil -extract id raw -o - - <<<"$result")
  status=$(plutil -extract status raw -o - - <<<"$result")
  if [[ "$status" != Accepted ]]; then
    xcrun notarytool log "$submission" "${NOTARY[@]}" >&2 || true
    die "notarization finished with status '$status' (submission $submission)"
  fi
  # The DMG is the outermost container, so its ticket covers the app inside.
  xcrun stapler staple "$DMG_TEMP"
  xcrun stapler validate "$DMG_TEMP"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG_TEMP"
fi
mv -f "$DMG_TEMP" "$DMG"
log "Built $APP"
log "Built $DMG"
