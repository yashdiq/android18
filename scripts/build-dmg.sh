#!/usr/bin/env bash
# Builds Android18.app from the release binary and packs a themed
# dist/Android18-<ver>.dmg: Retina background (scripts/dmg-background.swift),
# a 660x400 Finder window with the app and an /Applications drop target
# pre-arranged, and the app icon as the volume icon.
#
# No certificate is needed to build or use the DMG on your own Macs. When one
# exists it is used, for a stable signature across releases and the hardened
# runtime. Identity: $ANDROID18_SIGN_IDENTITY, else the first "Developer ID
# Application", else any local codesigning identity, else ad-hoc. A Developer
# ID is *only* about other people's Macs: Gatekeeper there rejects anything
# lesser (recipients can right-click → Open once). For that case set
# ANDROID18_NOTARIZE=1 (also needs `xcrun notarytool store-credentials
# android18`) to submit, wait and staple the DMG.
#
# The build is silent — nothing is installed, launched or opened on this
# Mac; the only output is the DMG path at the end. Mount it yourself and
# drag Android18 to /Applications. The Finder window layout (background,
# bounds, icon positions) ships as a checked-in .DS_Store template
# (scripts/assets/ds-store-template) that is copied onto the volume; Finder
# applies it when the DMG is mounted. Re-mint it only when the layout
# changes: mount a read-write volume named "Android18" holding
# .background/background.tiff, a stub "Android18.app" folder and an
# /Applications symlink, arrange it in Finder (660x400 window, 128px icons,
# background picture set, items positioned), then copy the volume's
# .DS_Store over the template.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
APP="dist/Android18.app"
DMG="dist/Android18-${VERSION}.dmg"
# Canonical icon: the same PNG platform.rs embeds for the runtime Dock icon,
# so the bundle .icns can never drift from what dev runs show.
ICON_SRC="crates/app/src/icons/AppIcon.png"
# Finder layout for the DMG window, minted once (see header); Finder applies
# it at mount time — the build itself never touches Finder.
TPL="scripts/assets/ds-store-template"

cargo build --release --locked -p android18-app
rm -rf "$APP" "$DMG"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/android18 "$APP/Contents/MacOS/android18"

# The bundle icon is a hard requirement — a generic-icon app must never ship.
scripts/make-icns.sh "$ICON_SRC" "$APP/Contents/Resources/Android18.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Android18</string>
  <key>CFBundleDisplayName</key><string>Android18</string>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleIdentifier</key><string>com.android18.desktop</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleExecutable</key><string>android18</string>
  <key>CFBundleIconFile</key><string>Android18</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSLocalNetworkUsageDescription</key><string>Android18 talks to your phone over the local network.</string>
</dict></plist>
PLIST

identity() {
    if [[ -n "${ANDROID18_SIGN_IDENTITY:-}" ]]; then
        printf '%s' "$ANDROID18_SIGN_IDENTITY"
        return
    fi
    local ids pick
    ids="$(security find-identity -v -p codesigning \
        | sed -n 's/^ *[0-9]*) [0-9A-F]* "\(.*\)"$/\1/p')"
    pick="$(grep -m1 '^Developer ID Application' <<<"$ids" || true)"
    [[ -n "$pick" ]] || pick="$(head -1 <<<"$ids")"
    printf '%s' "$pick"
}

NOTARIZE="${ANDROID18_NOTARIZE:-0}"
ID="$(identity)"
SIGN=(--force)
if [[ -n "$ID" ]]; then
    SIGN+=(--sign "$ID" --options runtime)
    # Notarization requires a secure timestamp; otherwise it is pure latency.
    if [[ "$NOTARIZE" == 1 ]]; then SIGN+=(--timestamp); else SIGN+=(--timestamp=none); fi
else
    SIGN+=(--sign - --timestamp=none)
    echo "build-dmg: no codesigning identity — signing ad-hoc (README → Package it)"
fi

codesign "${SIGN[@]}" "$APP"
codesign --verify --strict --verbose=1 "$APP" >/dev/null
# In-place rebuilds otherwise keep a stale Finder/Dock icon cache.
touch "$APP"
echo "signed with: ${ID:-ad-hoc}"

# ---- volume content ----------------------------------------------------------
STAGE="$(mktemp -d)"
MNT="/Volumes/Android18"
RW=""
cleanup() {
    hdiutil detach "$MNT" >/dev/null 2>&1 || true
    [[ -z "$RW" ]] || rm -rf "$RW"
    rm -rf "$STAGE"
}
trap cleanup EXIT

cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
# Dot-prefixed, so Finder keeps the staging background out of sight.
mkdir -p "$STAGE/.background"
scripts/dmg-background.swift \
    "$STAGE/.background/background.png" \
    "$STAGE/.background/background@2x.png" "$VERSION"
tiffutil -cathidpicheck \
    "$STAGE/.background/background.png" \
    "$STAGE/.background/background@2x.png" \
    -out "$STAGE/.background/background.tiff" >/dev/null

# ---- read-write staging image -------------------------------------------------
# diskutil (macOS 26+) builds it without the deprecated `hdiutil create`, but
# only as an APFS sparsebundle — that converts to UDZO fine. Older systems
# fall back to hdiutil; its deprecation warning is filtered because diskutil
# cannot make an HFS+ read-write image, so switching fully is not possible.
hdiutil detach "$MNT" >/dev/null 2>&1 || true
if diskutil image create blank --format UDSB --volumeName "Android18" \
    --fs APFS --size 64m "$STAGE/rw.sparsebundle" >/dev/null 2>&1; then
    RW="$STAGE/rw.sparsebundle"
else
    hdiutil create -volname "Android18" -fs HFS+ -format UDRW -size 64m \
        -ov "$STAGE/rw.dmg" >"$STAGE/create.log" 2>&1 \
        || { cat "$STAGE/create.log" >&2; exit 1; }
    grep -v "is deprecated" "$STAGE/create.log" >&2 || true
    RW="$STAGE/rw.dmg"
fi
# -nobrowse keeps even the staging mount off the Desktop.
hdiutil attach "$RW" -readwrite -noverify -noautoopen -nobrowse \
    -mountpoint "$MNT" >/dev/null

cp -R "$STAGE/Android18.app" "$STAGE/Applications" "$STAGE/.background" "$MNT/"
# The volume shows the app icon (the custom-icon flag needs SetFile from CLT).
cp "$APP/Contents/Resources/Android18.icns" "$MNT/.VolumeIcon.icns"
if command -v SetFile >/dev/null; then
    SetFile -a C "$MNT/.VolumeIcon.icns" || true
fi

# ---- Finder layout (checked-in .DS_Store template) -----------------------------
# 660x400 window, app left, /Applications right — applied by Finder when
# *you* mount the DMG, from a .DS_Store minted once by arranging a scratch
# "Android18" volume this way (see header). The build never drives Finder:
# opening the staged volume from AppleScript could launch the app straight
# off the staging image, and Finder holds the volume while it browses.
[[ -f "$TPL" ]] || { echo "build-dmg: missing $TPL — see header" >&2; exit 1; }
cp "$TPL" "$MNT/.DS_Store"

# Nothing Finder-side holds the volume, so this detaches immediately; the
# retry loop is only belt-and-braces.
for _ in 1 2 3; do
    hdiutil detach "$MNT" >/dev/null 2>&1 && break
    sleep 1
done
hdiutil detach -force "$MNT" >/dev/null 2>&1 || true

# ---- compress + sign ----------------------------------------------------------
if ! diskutil image create from "$RW" "${DMG%.dmg}" --format UDZO >/dev/null 2>&1 \
    || [[ ! -f "$DMG" ]]; then
    hdiutil convert "$RW" -format UDZO -imagekey zlib-level=9 -ov \
        -o "${DMG%.dmg}" >/dev/null
fi
codesign "${SIGN[@]}" "$DMG"

if [[ "$NOTARIZE" == 1 ]]; then
    xcrun notarytool submit "$DMG" --keychain-profile android18 --wait
    xcrun stapler staple "$APP"
    xcrun stapler staple "$DMG"
fi
echo "DMG: $PWD/$DMG"
