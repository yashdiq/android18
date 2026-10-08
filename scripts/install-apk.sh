#!/usr/bin/env bash
# Installs dist/android18-*.apk on the connected phone and launches it.
# A debug build (Android Studio Run) is signed with a different key, so it is
# removed first — Android refuses the update otherwise
# (INSTALL_FAILED_UPDATE_INCOMPATIBLE).
set -euo pipefail
cd "$(dirname "$0")/.."
APK="$(ls -t dist/android18-*.apk | head -1)"
PKG=com.android18.service
adb get-state >/dev/null
if adb install -r "$APK" 2>&1 | tee /tmp/android18-install.log | grep -q Success; then :; else
    if grep -q "UPDATE_INCOMPATIBLE\|signatures do not match" /tmp/android18-install.log; then
        echo "signature differs from the installed build; uninstalling it (app data is lost)"
        adb uninstall "$PKG"
        adb install "$APK"
    else
        cat /tmp/android18-install.log; exit 1
    fi
fi
adb shell monkey -p "$PKG" -c android.intent.category.LAUNCHER 1 >/dev/null
sleep 2
adb logcat -d -t 200 | grep -E "AndroidRuntime|FATAL" || echo "launched, no crash in logcat"
