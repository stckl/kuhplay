#!/usr/bin/env bash
# Baut KuhPlay für Linux: dist/kuhplay-<version>-linux-<arch>.tar.gz
# Aufruf: scripts/build-linux.sh [--no-cef]
set -euo pipefail

cd "$(dirname "$0")/.."

cef=1
case "${1:-}" in
  "") ;;
  --no-cef) cef=0 ;;
  *) echo "Aufruf: $0 [--no-cef]" >&2; exit 2 ;;
esac

[ "$(uname -s)" = "Linux" ] || { echo "Dieses Skript läuft nur auf Linux." >&2; exit 1; }
command -v cargo >/dev/null || { echo "cargo fehlt (https://rustup.rs)." >&2; exit 1; }

# Systembibliotheken vorab prüfen, sonst bricht der Build erst spät ab.
required="alsa gtk+-3.0" # CEF bringt seine Runtime selbst mit (cef/)
missing=""
for lib in $required; do
  pkg-config --exists "$lib" 2>/dev/null || missing="$missing $lib"
done
if [ -n "$missing" ]; then
  echo "Fehlende Bibliotheken:$missing" >&2
  echo "Debian/Ubuntu: sudo apt install build-essential pkg-config cmake ninja-build libasound2-dev libgtk-3-dev libnss3" >&2
  echo "Fedora:        sudo dnf install gcc pkgconf-pkg-config cmake ninja-build alsa-lib-devel gtk3-devel nss" >&2
  exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="kuhplay-${version}-linux-$(uname -m)"

# H4: DeckLink gehört seit G6 zum Release (SDK liegt unter third_party/).
if [ "$cef" = 1 ]; then
  cargo build --release --locked --features cef,decklink
else
  cargo build --release --locked --features decklink
fi

out="dist/$name"
rm -rf "$out" "$out.tar.gz"
mkdir -p "$out"
cp target/release/kuhplay README.md icon.png "$out/"
# HTML5-Renderer samt CEF-Runtime in cef/ neben dem Haupt-Binary.
if [ "$cef" = 1 ]; then
  target/release/kuhplay-cef-bundle "$out/cef"
fi
# H3: ffmpeg/ffprobe gebündelt neben das Binary – schlägt das Mitliefern fehl
# (Download, Prüfsumme, ffmpeg-Checks), bricht der Release-Build hier ab.
scripts/vendor-ffmpeg.sh "$out"
tar -czf "$out.tar.gz" -C dist "$name"

echo "Fertig: $out.tar.gz"
