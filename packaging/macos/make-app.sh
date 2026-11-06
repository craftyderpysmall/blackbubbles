#!/bin/sh
# Build BlackBubbles.app (the client) for this Mac with Tauri's bundler.
# Needs Rust and Node.js (brew install node).
set -eu
cd "$(dirname "$0")/../.."
(cd ui && npm ci)
# `tauri build` runs `npm run build` in ui/ first (beforeBuildCommand), then embeds it.
ui/node_modules/.bin/tauri build --bundles app
APP="target/release/bundle/macos/BlackBubbles.app"
# Ad-hoc sign so macOS remembers notification permission across launches.
codesign --force --deep --sign - "$APP"
echo "Built $APP; drag it to /Applications."
