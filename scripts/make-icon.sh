#!/usr/bin/env bash
# Renders assets/icon/icon.svg into assets/icon/AppIcon.icns.
#
# Usage: scripts/make-icon.sh [icon.svg] [AppIcon.icns]
#
# To change the app icon, replace assets/icon/icon.svg (a 1024 x 1024 SVG)
# and run this script; commit both files. Rendering uses rsvg-convert when it
# is installed (sharpest), otherwise macOS's own Quick Look (qlmanage) + sips.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SVG="${1:-$ROOT/assets/icon/icon.svg}"
ICNS="${2:-$ROOT/assets/icon/AppIcon.icns}"
[[ -f "$SVG" ]] || {
    echo "no such SVG: $SVG" >&2
    exit 1
}
command -v iconutil >/dev/null || {
    echo "iconutil not found (macOS only)" >&2
    exit 1
}

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"

# render <size> <out.png>
if command -v rsvg-convert >/dev/null; then
    echo "renderer: rsvg-convert"
    render() { rsvg-convert -w "$1" -h "$1" "$SVG" -o "$2"; }
else
    echo "renderer: qlmanage + sips"
    qlmanage -t -s 1024 -o "$WORK" "$SVG" >/dev/null 2>&1
    MASTER="$WORK/$(basename "$SVG").png"
    [[ -f "$MASTER" ]] || {
        echo "qlmanage could not render $SVG" >&2
        exit 1
    }
    render() { sips -z "$1" "$1" "$MASTER" --out "$2" >/dev/null; }
fi

for size in 16 32 128 256 512; do
    render "$size" "$ICONSET/icon_${size}x${size}.png"
    render "$((size * 2))" "$ICONSET/icon_${size}x${size}@2x.png"
done

iconutil -c icns "$ICONSET" -o "$ICNS"
echo "wrote $ICNS"
