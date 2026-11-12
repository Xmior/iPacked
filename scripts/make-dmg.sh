#!/bin/bash
# Builds iPacked and packs it into iPacked_<version>_<arch>.dmg with the custom
# disk (volume) icon. Tauri can't set a volume icon, and its own DMG step needs
# Finder automation permission, so the DMG is made here instead.
#
# Usage: scripts/make-dmg.sh            (from the app/ folder)
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=universal-apple-darwin
ARCH=universal
VERSION=$(sed -n 's/.*"version": "\(.*\)".*/\1/p' src-tauri/tauri.conf.json | head -1)

MACOSX_DEPLOYMENT_TARGET=12.0 npx tauri build --target "$TARGET"

BUNDLE="src-tauri/target/$TARGET/release/bundle"
APP="$BUNDLE/macos/iPacked.app"

# Ad-hoc sign every helper, then seal the whole bundle under one identity, so
# macOS remembers privacy permissions (Documents etc.) instead of asking for
# every helper call. Helpers in Resources aren't signed by Tauri, and an
# unsigned arm64 binary can't run at all on Apple Silicon.
for f in "$APP"/Contents/MacOS/* "$APP"/Contents/Resources/{fpdecrypt,tsschecker,bundle-icon}; do
  [ "$f" = "$APP/Contents/MacOS/ipacked" ] && continue
  codesign --force --sign - --identifier "com.ipacked.app.$(basename "$f")" "$f"
done
codesign --force --sign - --identifier com.ipacked.app "$APP"
codesign --verify --deep --strict "$APP"
OUT="$BUNDLE/dmg/iPacked_${VERSION}_${ARCH}.dmg"
mkdir -p "$BUNDLE/dmg"
rm -f "$OUT"

# create-dmg script that Tauri bundles; --skip-jenkins skips the Finder AppleScript.
DMG_SH=$(find src-tauri/target -name bundle_dmg.sh | head -1)
if [ -z "$DMG_SH" ]; then
  echo "bundle_dmg.sh not found: run 'npx tauri build --bundles dmg' once to get it." >&2
  exit 1
fi

"$DMG_SH" \
  --volname "iPacked" \
  --volicon src-tauri/icons/dmg-volume.icns \
  --window-size 660 400 \
  --icon "iPacked.app" 180 170 \
  --app-drop-link 480 170 \
  --skip-jenkins \
  "$OUT" "$APP"

hdiutil verify "$OUT"
echo "→ $OUT"
