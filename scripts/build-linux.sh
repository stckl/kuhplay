#!/usr/bin/env bash
# Baut KuhPlay für Linux: dist/kuhplay-<version>-linux-<arch>.tar.gz
# Aufruf: scripts/build-linux.sh [--no-webview]
set -euo pipefail

cd "$(dirname "$0")/.."

webview=1
case "${1:-}" in
  "") ;;
  --no-webview) webview=0 ;;
  *) echo "Aufruf: $0 [--no-webview]" >&2; exit 2 ;;
esac

[ "$(uname -s)" = "Linux" ] || { echo "Dieses Skript läuft nur auf Linux." >&2; exit 1; }
command -v cargo >/dev/null || { echo "cargo fehlt (https://rustup.rs)." >&2; exit 1; }

# Systembibliotheken vorab prüfen, sonst bricht der Build erst spät ab.
required="alsa gtk+-3.0"
[ "$webview" = 1 ] && required="$required webkit2gtk-4.1"
missing=""
for lib in $required; do
  pkg-config --exists "$lib" 2>/dev/null || missing="$missing $lib"
done
if [ -n "$missing" ]; then
  echo "Fehlende Bibliotheken:$missing" >&2
  echo "Debian/Ubuntu: sudo apt install build-essential pkg-config libasound2-dev libgtk-3-dev libwebkit2gtk-4.1-dev" >&2
  echo "Fedora:        sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel gtk3-devel webkit2gtk4.1-devel" >&2
  exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="kuhplay-${version}-linux-$(uname -m)"

if [ "$webview" = 1 ]; then
  cargo build --release --locked --features webview
else
  cargo build --release --locked
fi

out="dist/$name"
rm -rf "$out" "$out.tar.gz"
mkdir -p "$out"
cp target/release/kuhplay README.md icon.png "$out/"
# Die Engine sucht den WebView-Prozess neben dem Haupt-Binary.
if [ "$webview" = 1 ]; then
  cp target/release/kuhplay-webview "$out/"
fi
tar -czf "$out.tar.gz" -C dist "$name"

command -v ffmpeg >/dev/null || echo "Hinweis: ffmpeg fehlt im PATH, Video-Cues brauchen es zur Laufzeit." >&2
echo "Fertig: $out.tar.gz"

# G7: ffmpeg/ffprobe gebündelt neben das Binary.
if [ -x scripts/vendor-ffmpeg.sh ]; then
  scripts/vendor-ffmpeg.sh dist/linux/ || echo "Hinweis: vendor-ffmpeg fehlgeschlagen – ffmpeg/ffprobe müssen im PATH liegen." >&2
fi
