#!/usr/bin/env bash
# G7: ffmpeg + ffprobe als statische LGPL-Builds neben die App legen.
# Quelle: gyan.dev "full"-Builds (LGPL, ohne GPL-/Nonfree-Teile; Hardware-Decoder
# wie NVENC/QSV/VAAPI sind in diesen Builds enthalten). Lizenztext + Quellenhinweis
# liegen danach in <ziel>/LICENSE-ffmpeg.txt.
#
# Aufruf: scripts/vendor-ffmpeg.sh <zielverzeichnis>
#   macOS:  Contents/MacOS des .app-Bundles
#   Win:    Verzeichnis neben kuhplay.exe
#   Linux:  Verzeichnis neben dem Binary
set -euo pipefail

dest="${1:?Aufruf: vendor-ffmpeg.sh <zielverzeichnis>}"
mkdir -p "$dest"

os=$(uname -s)
case "$os" in
  Darwin) plat=macos; zip_url="https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" ;;
  Linux)  plat=linux; zip_url="https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" ;;
  *) echo "Windows: bitte scripts/vendor-ffmpeg.ps1 nutzen." >&2; exit 1 ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
echo "Lade ffmpeg (LGPL, gyan.dev full/essentials) …"
curl -fL --retry 3 -o "$work/ffmpeg.zip" "$zip_url"
unzip -o -q "$work/ffmpeg.zip" -d "$work"

bin_dir=$(find "$work" -type d -name bin | head -1)
install -m 0755 "$bin_dir/ffmpeg" "$dest/ffmpeg"
install -m 0755 "$bin_dir/ffprobe" "$dest/ffprobe"

cat > "$dest/LICENSE-ffmpeg.txt" <<'EOF'
ffmpeg/ffprobe in diesem Verzeichnis: statische Builds von https://www.gyan.dev/ffmpeg/builds/
Lizenz: GNU LGPL v2.1 oder später (Build ohne GPL- und Nonfree-Komponenten).
Quellen: https://github.com/FFmpeg/FFmpeg (Tag siehe Release auf gyan.dev).
Lizenztext: https://www.gnu.org/licenses/lgpl-2.1.html
EOF

echo "OK: ffmpeg + ffprobe + LICENSE-ffmpeg.txt nach $dest"
