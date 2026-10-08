#!/usr/bin/env bash
# Builds, verifies and copies the signed release APK to dist/.
# Signing: android-service/keystore.properties (see keystore.properties.example)
# or ANDROID18_KS_FILE / ANDROID18_KS_PASS / ANDROID18_KEY_ALIAS env vars.
set -euo pipefail
cd "$(dirname "$0")/../android-service"

SDK="$(sed -n 's/^sdk.dir=//p' local.properties 2>/dev/null || true)"
export ANDROID_HOME="${ANDROID_HOME:-${SDK:-$HOME/Library/Android/sdk}}"
[[ -z "${JAVA_HOME:-}" && -x /usr/libexec/java_home ]] && JAVA_HOME="$(/usr/libexec/java_home -v 17 2>/dev/null || true)" && export JAVA_HOME

./gradlew --console=plain :app:assembleRelease

APK="$(ls app/build/outputs/apk/release/*.apk | head -1)"
BT="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
echo "build-tools: $BT"
"$BT/apksigner" verify --verbose "$APK" | head -5   # fails on unsigned APK
"$BT/zipalign" -c -P 16 4 "$APK"

VERSION="$(sed -n 's/.*versionName = "\(.*\)"/\1/p' app/build.gradle.kts | head -1)"
mkdir -p ../dist
OUT="../dist/android18-${VERSION}.apk"
cp "$APK" "$OUT"
echo "APK: $(cd .. && pwd)/dist/$(basename "$OUT")"
