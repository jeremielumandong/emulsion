#!/usr/bin/env bash
# Regenerate platform icon sizes from the supplied artwork. Requires ImageMagick.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ICON_DIR="$ROOT_DIR/assets/icons"
magick "$ICON_DIR/emulsion-source.png" -resize 512x512 "$ICON_DIR/emulsion.png"
magick "$ICON_DIR/emulsion-source.png" -define icon:auto-resize=256,128,64,48,32,24,16 "$ICON_DIR/emulsion.ico"
