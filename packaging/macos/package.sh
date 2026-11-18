#!/usr/bin/env bash
# usage: package.sh VERSION TARGET   -> dist/*.zip and dist/*.dmg
set -euo pipefail
V=$1; T=$2; ROOT=$(cd "$(dirname "$0")/../.." && pwd); REL="$ROOT/target/$T/release"
case $T in aarch64*) ARCH=aarch64;; *) ARCH=x86_64;; esac
OUT="$ROOT/dist"; mkdir -p "$OUT"; S=$(mktemp -d)
APP="$S/guitar-fx.app"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$REL/guitar-fx-ui" "$APP/Contents/MacOS/guitar-fx-ui"; cp "$REL/guitar-fx" "$APP/Contents/MacOS/guitar-fx"
cp -r "$ROOT/presets" "$APP/Contents/Resources/presets"
ICONSET="$S/icon.iconset"; mkdir "$ICONSET"
for z in 16 32 64 128 256; do sips -z $z $z "$ROOT/packaging/guitar-fx.png" --out "$ICONSET/icon_${z}x${z}.png" >/dev/null; done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/guitar-fx.icns"
cat > "$APP/Contents/Info.plist" <<P
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>guitar-fx</string>
<key>CFBundleIdentifier</key><string>io.github.alfu32.guitar-fx</string>
<key>CFBundleExecutable</key><string>guitar-fx-ui</string>
<key>CFBundleIconFile</key><string>guitar-fx</string>
<key>CFBundleShortVersionString</key><string>$V</string>
<key>CFBundleVersion</key><string>$V</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSMicrophoneUsageDescription</key><string>guitar-fx processes your guitar input.</string>
</dict></plist>
P
codesign --force --deep --sign - "$APP"   # ad-hoc: runs locally, not notarised
ditto -c -k --keepParent "$APP" "$OUT/guitar-fx-$V-macos-$ARCH.zip"
ln -s /Applications "$S/Applications"
hdiutil create -volname "guitar-fx $V" -srcfolder "$S" -ov -format UDZO "$OUT/guitar-fx-$V-macos-$ARCH.dmg"
