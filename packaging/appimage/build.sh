#!/bin/bash
# Assemble the SwiftSwitcher AppDir and pack it into an AppImage.
#
#   packaging/appimage/build.sh [--appdir-only]
#
# Environment:
#   BIN                 built release binary (default: target/release/swiftswitcher)
#   APPIMAGETOOL        path to an appimagetool executable (required unless --appdir-only)
#   APPIMAGETOOL_SHA256 if set, the tool is verified against this sha256 first
#
# GTK4 and libadwaita are not bundled; they come from the host.
set -euo pipefail

appdir_only=0
case "${1:-}" in
    "") ;;
    --appdir-only) appdir_only=1 ;;
    *) echo "usage: $0 [--appdir-only]" >&2; exit 2 ;;
esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

bin="${BIN:-target/release/swiftswitcher}"
[ -x "$bin" ] || { echo "error: binary '$bin' not found; run cargo build --release -p swiftswitcher" >&2; exit 1; }

version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' crates/daemon/Cargo.toml | head -n1)"
[ -n "$version" ] || { echo "error: cannot read version from crates/daemon/Cargo.toml" >&2; exit 1; }

app_id=io.github.vipruk.SwiftSwitcher
appdir="target/SwiftSwitcher.AppDir"
res="$appdir/usr/share/swiftswitcher"
ext=swiftswitcher@vipruk.github.io

rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$res/extension/$ext" "$res/udev" "$appdir/usr/share/metainfo"

install -m 755 "$bin" "$appdir/usr/bin/swiftswitcher"
install -m 755 packaging/appimage/AppRun "$appdir/AppRun"
cp -r data/icons "$res/icons"
install -m 644 extension/"$ext"/*.js extension/"$ext"/metadata.json "$res/extension/$ext/"
install -m 644 packaging/udev/70-swiftswitcher.rules "$res/udev/"
install -m 644 data/$app_id.desktop "$appdir/$app_id.desktop"
install -m 644 data/$app_id.metainfo.xml "$appdir/usr/share/metainfo/$app_id.metainfo.xml"
install -m 644 data/icons/hicolor/scalable/apps/$app_id.svg "$appdir/$app_id.svg"
ln -s "$app_id.svg" "$appdir/.DirIcon"

echo "AppDir ready: $appdir"
[ "$appdir_only" -eq 1 ] && exit 0

tool="${APPIMAGETOOL:-}"
[ -n "$tool" ] || { echo "error: set APPIMAGETOOL to the appimagetool executable" >&2; exit 1; }
[ -x "$tool" ] || { echo "error: APPIMAGETOOL '$tool' is not executable" >&2; exit 1; }
if [ -n "${APPIMAGETOOL_SHA256:-}" ]; then
    echo "$APPIMAGETOOL_SHA256  $tool" | sha256sum -c -
fi

out="target/SwiftSwitcher-$version-x86_64.AppImage"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" "$appdir" "$out"
echo "Built $out"
