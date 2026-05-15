#!/usr/bin/env bash
# regenerate-app-icons.sh
#
# Regenerate all Tauri app icons (macOS, iOS, Android, Windows) from a single
# 1024x1024 master image. Fixes two iOS-specific problems:
#
#   1. iOS rejects icons with alpha. Compose the artwork over an opaque
#      background colour (--bg) before exporting.
#   2. iOS auto-masks icons to a rounded square. If the artwork fills the
#      full 1024 frame, the rounded mask shaves the corners off the art.
#      Pad the artwork to leave ~10% safe zone on every side.
#
# Requirements: ImageMagick 7 (`magick`) + the Tauri CLI (`tauri icon`).
# Install on macOS: brew install imagemagick && (cd tauri && npm i)
#
# Usage:
#   scripts/regenerate-app-icons.sh public/logo.png
#   scripts/regenerate-app-icons.sh public/logo.png --bg "#1F2A44"
#   scripts/regenerate-app-icons.sh public/logo.png --padding 0.12
#
# The result lands in tauri/src-tauri/icons/. Re-run after editing the master.

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <source-png> [--bg '#RRGGBB'] [--padding 0.10]" >&2
  exit 64
fi

SRC="$1"; shift || true
BG="#1F2A44"
PADDING="0.10"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bg)      BG="$2"; shift 2;;
    --padding) PADDING="$2"; shift 2;;
    *) echo "unknown arg: $1" >&2; exit 64;;
  esac
done

if [[ ! -f "$SRC" ]]; then
  echo "source not found: $SRC" >&2
  exit 66
fi

if ! command -v magick >/dev/null 2>&1; then
  echo "ImageMagick 7 not found. Install with: brew install imagemagick" >&2
  exit 69
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAURI_DIR="$REPO_ROOT/tauri"
ICON_DIR="$TAURI_DIR/src-tauri/icons"
MASTER="$ICON_DIR/icon.png"

mkdir -p "$ICON_DIR"

# Build a square master with the artwork inset by PADDING on every side,
# over an opaque background. Output is 1024x1024 PNG with NO alpha.
INSET=$(awk -v p="$PADDING" 'BEGIN { printf "%d", 1024 * (1 - 2*p) }')

echo "Building master icon (1024x1024, bg=$BG, padding=$PADDING => artwork ${INSET}x${INSET}):"
echo "  src    : $SRC"
echo "  output : $MASTER"

magick "$SRC" \
  -background "$BG" -alpha remove -alpha off \
  -resize "${INSET}x${INSET}" \
  -gravity center -extent 1024x1024 \
  -background "$BG" -alpha off \
  -define png:color-type=2 \
  "$MASTER"

# Sanity: confirm no alpha channel made it through (iOS App Store rejects alpha).
if magick identify -format "%A" "$MASTER" | grep -qiv "false\|undefined"; then
  echo "WARNING: master still appears to have an alpha channel. iOS may reject it." >&2
fi

# Now have Tauri regenerate all platform-specific icons from the master.
echo "Running tauri icon..."
cd "$TAURI_DIR"
npx @tauri-apps/cli icon "$MASTER"

echo
echo "Done. Regenerated icons under: $ICON_DIR"
echo "Commit the master and the platform-specific outputs together."
