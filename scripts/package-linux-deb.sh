#!/bin/bash
# Build a Debian package containing the Rust binary, a desktop entry, and the icon.
set -euo pipefail

project=$(cd "$(dirname "$0")/.." && pwd)
binary=${1:?Supply the path to the built Rust executable}
version=${2:-0.2.0}
output=${3:-"$project/dist/Super-OpenCode-linux-x64.deb"}
arch=$(dpkg --print-architecture)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pkg="$work/super-opencode"
mkdir -p "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/applications" "$pkg/usr/share/pixmaps"

install -m 755 "$binary" "$pkg/usr/bin/super-opencode"
install -m 644 "$project/assets/logo.png" "$pkg/usr/share/pixmaps/super-opencode.png"

cat > "$pkg/usr/share/applications/super-opencode.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Super OpenCode
Comment=Desktop app for OpenCode using free models
Exec=super-opencode
Icon=super-opencode
Categories=Development;IDE;
Terminal=false
DESKTOP

cat > "$pkg/DEBIAN/control" <<CONTROL
Package: super-opencode
Version: $version
Section: devel
Priority: optional
Architecture: $arch
Maintainer: desmondknunoo <desmondknunoo@users.noreply.github.com>
Depends: libc6, libasound2, libgl1, libxkbcommon0
Homepage: https://github.com/desmondknunoo/super-opencode
Description: Desktop app for OpenCode using free models
 Super OpenCode runs OpenCode in your project folder and shows each step
 as it happens. It only offers models priced at zero dollars.
CONTROL

mkdir -p "$(dirname "$output")"
dpkg-deb --build --root-owner-group "$pkg" "$output"
dpkg-deb --info "$output" >/dev/null
printf 'Wrote %s\n' "$output"
