#!/usr/bin/env bash
# Dependency-free dev loop: watch source/asset mtimes, rebuild, relaunch.
#
# This is the "hot reload" for a native app — `cargo run` builds once and
# never watches. `find -newer` against a stamp file replaces fswatch; icon
# SVGs are re-read from disk by rust-embed in debug builds, so a relaunch is
# all a glyph tweak needs, code changes get a full rebuild.
#
# The binary always launches from inside target/debug/Android18.app, a
# generated dev bundle: a bare cargo executable has no Info.plist, so every
# macOS surface that resolves icons from the bundle (Stage Manager, Mission
# Control grouping, ⌘-Tab) would show the generic executable icon — only the
# Dock tile honors the runtime setApplicationIconImage. Inside the wrapper
# they all resolve the bundled .icns.
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="target/debug/android18"
APP="target/debug/Android18.app"
WRAPPER_BIN="$APP/Contents/MacOS/android18"
ICON="crates/app/src/icons/AppIcon.png"
STAMP="target/.dev-stamp"
pid=""

# Debug binaries are ad-hoc signed: every rebuild changes the code hash, so
# macOS treats the next run as a brand-new app. Re-signing the wrapper with
# a stable local identity keeps the dev bundle closest to the release DMG
# (constant identity, warm icon/signature caches). The wrapper bundle (not
# the bare binary) is what gets signed: identity-signed code launched from
# inside a .app must carry a sealed CodeResources envelope, or the kernel
# kills it with "Taskgated Invalid Signature".
identity() {
    security find-identity -v -p codesigning \
        | sed -n 's/^ *[0-9]*) [0-9A-F]* "\(.*\)"$/\1/p' | head -1
}

sign() {
    local id
    id="$(identity)"
    if [[ -z "$id" ]]; then
        echo "android18: no codesigning identity — continuing ad-hoc signed"
        return 0
    fi
    if codesign --force --sign "$id" --timestamp=none "$APP" 2>/dev/null; then
        echo "android18: signed with \"$id\""
    else
        echo "android18: codesign failed — continuing ad-hoc signed"
    fi
}

# (Re)builds the dev bundle: Info.plist + full .icns (only when the canonical
# icon changed) + a copy of the freshly built binary. sign() then seals the
# whole bundle — see there for why the bundle, not the bare binary, must be
# what gets signed.
wrap() {
    mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
    if [[ ! -f "$APP/Contents/Resources/Android18.icns" \
          || "$ICON" -nt "$APP/Contents/Resources/Android18.icns" ]]; then
        scripts/make-icns.sh "$ICON" "$APP/Contents/Resources/Android18.icns"
    fi
    cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Android18</string>
  <key>CFBundleDisplayName</key><string>Android18</string>
  <key>CFBundleIdentifier</key><string>com.android18.dev</string>
  <key>CFBundleExecutable</key><string>android18</string>
  <key>CFBundleIconFile</key><string>Android18</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>dev</string>
  <key>CFBundleVersion</key><string>dev</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
    cp "$BIN" "$WRAPPER_BIN"
    # In-place rebuilds otherwise keep a stale Dock/Finder icon cache.
    touch "$APP"
}

changed() {
    find crates Cargo.toml Cargo.lock rust-toolchain.toml \
        \( -name '*.rs' -o -name '*.toml' -o -name '*.svg' -o -name '*.ttf' -o -name '*.png' \) \
        -newer "$STAMP" -print -quit 2>/dev/null || true
}

stop() {
    if [[ -n "$pid" ]]; then
        kill "$pid" 2>/dev/null || true
        for _ in 1 2 3 4 5 6 7 8 9 10; do
            kill -0 "$pid" 2>/dev/null || break
            sleep 0.1
        done
        kill -9 "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
        pid=""
    fi
}

launch() {
    if cargo build -p android18-app; then
        stop
        wrap
        sign
        "$WRAPPER_BIN" & pid=$!
        echo "android18: running (pid $pid, $APP) — ⌃C to stop"
    fi
}

mkdir -p target
touch "$STAMP"
echo "android18: watching crates/, Cargo.toml, Cargo.lock for changes"
launch

trap 'stop; exit 0' INT TERM
while sleep 1; do
    if [[ -n "$(changed)" ]]; then
        # Re-stamp before building so files saved during the build re-trigger,
        # and debounce so an editor "save all" fires one rebuild.
        touch "$STAMP"
        sleep 0.4
        echo "android18: change detected — rebuilding"
        launch
    fi
done
