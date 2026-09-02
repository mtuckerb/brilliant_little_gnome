#!/bin/bash
# Build the App Store iOS export of Brilliant and upload it to TestFlight.
# Local-only workflow (no CI): runs on a Mac with Xcode, mirroring
# release-desktop.sh's role for the desktop OTA channel.
#
# Usage: scripts/release-testflight.sh
# Requires:
#   - Xcode with the iOS platform installed (xcrun/xcodebuild)
#   - APPLE_DEVELOPMENT_TEAM   your 10-character Apple Developer Team ID
#   - ONE of these App Store Connect credential sets:
#       API key (preferred):
#         APP_STORE_CONNECT_KEY_ID     key id, e.g. ABC123XYZ0
#         APP_STORE_CONNECT_ISSUER_ID  issuer UUID
#         plus AuthKey_<KEY_ID>.p8 in ~/.appstoreconnect/private_keys/
#         (altool also checks ./private_keys and ~/private_keys)
#       Apple ID fallback:
#         APPLE_ID             the account email
#         APPLE_APP_PASSWORD   an app-specific password from appleid.apple.com
#
# Every upload needs a version App Store Connect hasn't seen for iOS. The
# version comes from src-tauri/tauri.conf.json — if the upload is rejected as
# a duplicate, bump it there (chore(release) commit) and rerun.
set -euo pipefail
cd "$(dirname "$0")/.."   # -> tauri/

{ [ "$(uname -s)" = "Darwin" ] && command -v xcrun >/dev/null; } \
  || { echo "This script needs macOS with Xcode (xcrun not found)."; exit 1; }
: "${APPLE_DEVELOPMENT_TEAM:?APPLE_DEVELOPMENT_TEAM must be set (10-char Apple Team ID)}"

VER="$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")"
IPA="src-tauri/gen/apple/build/arm64/Brilliant.ipa"

# Pick App Store Connect credentials: API key when configured, else Apple ID
# with an app-specific password. @env: keeps the password out of `ps` output.
if [ -n "${APP_STORE_CONNECT_KEY_ID:-}" ] && [ -n "${APP_STORE_CONNECT_ISSUER_ID:-}" ]; then
  CREDS=(--apiKey "$APP_STORE_CONNECT_KEY_ID" --apiIssuer "$APP_STORE_CONNECT_ISSUER_ID")
  echo "==> Using App Store Connect API key $APP_STORE_CONNECT_KEY_ID"
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
  CREDS=(--username "$APPLE_ID" --password @env:APPLE_APP_PASSWORD)
  echo "==> Using Apple ID $APPLE_ID (app-specific password)"
else
  echo "No App Store Connect credentials found." >&2
  echo "Set APP_STORE_CONNECT_KEY_ID + APP_STORE_CONNECT_ISSUER_ID (with the" >&2
  echo "AuthKey .p8 in ~/.appstoreconnect/private_keys/), or APPLE_ID +" >&2
  echo "APPLE_APP_PASSWORD (app-specific password)." >&2
  exit 1
fi

echo "==> Building App Store IPA for $VER"
npm run ios:build:app-store

[ -f "$IPA" ] || { echo "IPA missing at $IPA — check the build output above."; exit 1; }

echo "==> Validating $IPA against App Store Connect"
xcrun altool --validate-app -f "$IPA" -t ios "${CREDS[@]}"

echo "==> Uploading Brilliant $VER to TestFlight"
xcrun altool --upload-app -f "$IPA" -t ios "${CREDS[@]}"

echo "==> Done. App Store Connect is processing the build; it shows up in"
echo "    TestFlight once processing finishes (usually ~15 minutes). A first"
echo "    upload of a version may ask for export-compliance answers there."
