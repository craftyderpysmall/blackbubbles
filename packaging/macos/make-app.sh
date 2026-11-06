#!/bin/sh
# Build BlackBubbles.app (the client) for this Mac.
set -eu
cd "$(dirname "$0")/../.."
cargo build --release -p bb-client
APP="target/release/BlackBubbles.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/blackbubbles "$APP/Contents/MacOS/blackbubbles"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>blackbubbles</string>
    <key>CFBundleIdentifier</key><string>com.craftyderpysmall.blackbubbles</string>
    <key>CFBundleName</key><string>BlackBubbles</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$APP"
echo "Built $APP — drag it to /Applications."
