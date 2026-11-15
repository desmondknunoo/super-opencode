#!/bin/bash
set -euo pipefail
project=$(cd "$(dirname "$0")/.." && pwd)
binary=${1:?Supply the path to the built Rust executable}
destination=${2:-"$project/dist"}
app="$destination/Super OpenCode.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$binary" "$app/Contents/MacOS/SuperOpenCode"
cp "$project/assets/AppIcon.icns" "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>SuperOpenCode</string>
  <key>CFBundleIdentifier</key><string>local.super-opencode</string>
  <key>CFBundleName</key><string>Super OpenCode</string>
  <key>CFBundleDisplayName</key><string>Super OpenCode</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.2.0</string>
  <key>CFBundleVersion</key><string>2</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Super OpenCode turns what you say into a task. Speech is transcribed on this Mac and is not sent anywhere.</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$app"
codesign --verify --strict "$app"
