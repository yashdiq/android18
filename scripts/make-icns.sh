#!/usr/bin/env bash
# make-icns.sh <source.png> <output.icns>
#
# Builds a complete macOS .icns from a square PNG (>= 1024x1024). Only the
# documented .iconset member names are emitted (16/32/128/256/512 points
# plus @2x); names like icon_64x64.png are not in the spec even where
# iconutil happens to accept them. Shared by scripts/dev.sh (dev bundle) and
# scripts/build-dmg.sh (release bundle) so both icons always match the
# canonical crates/app/src/icons/AppIcon.png.
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <source.png> <output.icns>" >&2
    exit 64
fi
SRC=$1
OUT=$2

if [[ ! -f "$SRC" ]]; then
    echo "make-icns: icon source not found: $SRC" >&2
    exit 66
fi

W=$(sips -g pixelWidth "$SRC" | awk '/pixelWidth:/{print $2}')
H=$(sips -g pixelHeight "$SRC" | awk '/pixelHeight:/{print $2}')
if [[ -z "$W" || -z "$H" || "$W" -lt 1024 || "$H" -lt 1024 ]]; then
    echo "make-icns: $SRC is ${W:-?}x${H:-?}; need >= 1024x1024 (upscaling looks bad)" >&2
    exit 65
fi

WORK="$(mktemp -d)"
SET="$WORK/icon.iconset"
mkdir -p "$SET"
trap 'rm -rf "$WORK"' EXIT

# Documented members: 16, 32, 128, 256, 512 pt + @2x (32, 64, 256, 512, 1024 px).
for s in 16 32 128 256 512; do
    sips -z "$s" "$s" "$SRC" --out "$SET/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) "$SRC" --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done

if [[ "$OUT" == */* ]]; then
    mkdir -p "${OUT%/*}"
fi
iconutil -c icns "$SET" -o "$OUT"
