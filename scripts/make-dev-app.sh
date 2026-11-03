#!/bin/sh
# Dev-.app für macOS: KuhPlay + CEF-Runtime in korrektem Bundle-Layout
# (CEF crasht ohne .app mit SIGTRAP). CEF_DIST zeigt auf die entpackte
# Standard-Distribution (mit icudtl.dat), z. B. von cef-builds.spotifycdn.com.
# Aufruf: scripts/make-dev-app.sh [CEF_DIST_DIR]
set -e
CEF_DIST="${1:-/tmp/cef_binary_156.0.15+gcf671c0+chromium-156.0.8078.80_macosarm64/Release}"
APP="${2:-/Users/stocke/KuhPlay-dev.app}"
cargo build --release --locked --features cef,decklink
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"
cp target/release/kuhplay target/release/kuhplay-cef "$APP/Contents/MacOS/"
cp -R "$CEF_DIST/Chromium Embedded Framework.framework" "$APP/Contents/Frameworks/"
cp "$CEF_DIST/Chromium Embedded Framework.framework/Resources/"icudtl.dat "$CEF_DIST/Chromium Embedded Framework.framework/Resources/"*.pak "$APP/Contents/Resources/"
cp -R "$CEF_DIST/Chromium Embedded Framework.framework/Resources/locales" "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleName</key><string>KuhPlay</string><key>CFBundleExecutable</key><string>kuhplay</string><key>CFBundleIdentifier</key><string>dev.kuhplay</string><key>CFBundleVersion</key><string>0.1.2</string></dict></plist>
PLIST
echo "Dev-App: $APP"
