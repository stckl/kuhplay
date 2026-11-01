#!/usr/bin/env bash
# Baut KuhPlay für macOS: dist/KuhPlay.app und ein Zip davon.
# Aufruf: scripts/build-mac.sh [--no-webview]
set -euo pipefail

cd "$(dirname "$0")/.."

webview=1
case "${1:-}" in
  "") ;;
  --no-webview) webview=0 ;;
  *) echo "Aufruf: $0 [--no-webview]" >&2; exit 2 ;;
esac

[ "$(uname -s)" = "Darwin" ] || { echo "Dieses Skript läuft nur auf macOS." >&2; exit 1; }
command -v cargo >/dev/null || { echo "cargo fehlt (https://rustup.rs)." >&2; exit 1; }

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)

if [ "$webview" = 1 ]; then
  cargo build --release --locked --features webview
else
  cargo build --release --locked
fi

app=dist/KuhPlay.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/kuhplay "$app/Contents/MacOS/"
# Die Engine sucht den WebView-Prozess neben dem Haupt-Binary.
if [ "$webview" = 1 ]; then
  cp target/release/kuhplay-webview "$app/Contents/MacOS/"
fi
# G7: ffmpeg/ffprobe gebündelt ins App-Bundle (falls vorhanden, sonst PATH-Hinweis).
if [ -x scripts/vendor-ffmpeg.sh ]; then
  scripts/vendor-ffmpeg.sh "$app/Contents/MacOS" || echo "Hinweis: vendor-ffmpeg fehlgeschlagen – ffmpeg/ffprobe müssen im PATH liegen." >&2
fi

# App-Icon: icon.png ist nicht quadratisch, iconutil braucht Quadrate.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
iconset="$work/KuhPlay.iconset"
mkdir "$iconset"
side=$(sips -g pixelWidth -g pixelHeight icon.png | awk '/pixel/ {print $2}' | sort -n | tail -1)
sips --padToHeightWidth "$side" "$side" icon.png --out "$work/square.png" >/dev/null
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$work/square.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$work/square.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/KuhPlay.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>KuhPlay</string>
  <key>CFBundleDisplayName</key><string>KuhPlay</string>
  <key>CFBundleIdentifier</key><string>com.kuhplay.app</string>
  <key>CFBundleExecutable</key><string>kuhplay</string>
  <key>CFBundleIconFile</key><string>KuhPlay</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc-Signatur: läuft auf diesem Mac. Auf anderen Macs braucht es zusätzlich
# eine Developer-ID und Notarisierung, sonst blockiert Gatekeeper den Start.
codesign --force --deep --sign - "$app"

zip="dist/kuhplay-${version}-macos-${arch}.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"

command -v ffmpeg >/dev/null || [ -x "$app/Contents/MacOS/ffmpeg" ] || echo "Hinweis: ffmpeg/ffprobe weder gebündelt noch im PATH – Video-Cues brauchen eines davon." >&2
echo "Fertig: $app und $zip"
