#!/bin/bash
# Build, sign, and publish a desktop OTA release of Brilliant to GitHub Releases.
# The app's updater endpoint is releases/latest/download/latest.json, so every
# release must include the `latest.json` manifest + the signed .app.tar.gz.
#
# Usage: scripts/release-desktop.sh [version]   (defaults to tauri.conf version)
# Requires: ~/.tauri/brilliant-updater.key (updater private key, empty password),
#           a "Developer ID Application" cert, and gh authenticated.
set -euo pipefail
cd "$(dirname "$0")/.."   # -> tauri/

REPO="mtuckerb/brilliant_little_gnome"
VER="${1:-$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")}"
TAG="v$VER"
KEY="$HOME/.tauri/brilliant-updater.key"
SIGN_ID="Developer ID Application: Tucker Bradford (QDWAV324SU)"

[ -f "$KEY" ] || { echo "missing updater key: $KEY"; exit 1; }

# Build the .app + updater artifact FIRST, on its own. The updater tarball is
# the only thing the OTA endpoint needs, and bundling it separately means a
# flaky `bundle_dmg.sh` (hdiutil detach failures) can no longer abort the build
# before the .app.tar.gz is produced — which is exactly what broke the 2.0.6
# release.
echo "==> Building signed desktop bundle $VER (app + updater)"
TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
APPLE_SIGNING_IDENTITY="$SIGN_ID" \
  npm run tauri -- build --target universal-apple-darwin --bundles app \
    --config '{"productName":"Brilliant Desktop"}'

# DMG is a nice-to-have for fresh installs; never let it fail the release.
echo "==> Building DMG (best-effort)"
TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
APPLE_SIGNING_IDENTITY="$SIGN_ID" \
  npm run tauri -- build --target universal-apple-darwin --bundles dmg \
    --config '{"productName":"Brilliant Desktop"}' || echo "    DMG bundling failed — continuing (OTA artifact is unaffected)"

BUNDLE="src-tauri/target/universal-apple-darwin/release/bundle/macos"
ART="$BUNDLE/Brilliant Desktop.app.tar.gz"
SIG="$ART.sig"
[ -f "$ART" ] || { echo "updater artifact missing: $ART"; exit 1; }
[ -f "$SIG" ] || { echo "signature missing: $SIG (createUpdaterArtifacts + signing key?)"; exit 1; }

ASSET="Brilliant-Desktop-$VER-universal.app.tar.gz"   # no spaces -> stable URL
cp "$ART" "/tmp/$ASSET"
URL="https://github.com/$REPO/releases/download/$TAG/$ASSET"

python3 - "$VER" "$SIG" "$URL" > /tmp/latest.json <<'PY'
import json, sys, datetime
ver, sigpath, url = sys.argv[1:4]
plat = {"signature": open(sigpath).read().strip(), "url": url}
print(json.dumps({
    "version": ver,
    "notes": f"Brilliant desktop {ver}",
    "pub_date": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    # Universal bundle: both arches point at the same artifact.
    "platforms": {"darwin-aarch64": plat, "darwin-x86_64": plat},
}, indent=2))
PY

echo "==> Publishing $TAG to GitHub Releases"
if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
    gh release upload "$TAG" "/tmp/$ASSET" "/tmp/latest.json" --repo "$REPO" --clobber
else
    gh release create "$TAG" "/tmp/$ASSET" "/tmp/latest.json" \
        --repo "$REPO" --title "Brilliant $VER" --notes "Desktop OTA release $VER"
fi
echo "==> Done. Installed apps will see $VER via the updater endpoint."
