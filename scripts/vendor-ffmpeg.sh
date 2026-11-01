#!/usr/bin/env bash
# H3: ffmpeg + ffprobe als LGPL-Builds neben die App legen.
#
# Quellen (gepinnt, mit SHA-256 – keine beweglichen "release"/"latest"-Links):
#   Linux x86_64/arm64: BtbN/FFmpeg-Builds, LGPL-Variante (statisch, ohne GPL-/Nonfree-
#     Komponenten: kein --enable-gpl, --disable-libx264/-libx265), Release-Tag
#     autobuild-2026-10-31-12-59, Hashes aus dessen checksums.sha256.
#   macOS: eigener Build aus dem offiziellen Quell-Tarball ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
#     mit --disable-gpl --disable-nonfree --disable-x264 --disable-x265 (damit reines LGPL).
#
# Nach dem Installieren wird geprüft: SHA-256, `ffmpeg -version` (Version + Build-Config,
# kein --enable-gpl/--enable-nonfree/--enable-libx264/--enable-libx265), Hardware-Beschleuniger
# (`-hwaccels` nicht leer) und ein Decode-Smoke-Test. Build-Config und Lizenzhinweise landen
# als BUILD-ffmpeg.txt / LICENSE-ffmpeg.txt im Zielverzeichnis.
# Schlägt etwas fehl, bricht das Skript mit Exit 1 ab – der Release-Build muss dann abbrechen.
#
# Aufruf: scripts/vendor-ffmpeg.sh <zielverzeichnis>
#   macOS:  Contents/MacOS des .app-Bundles
#   Linux:  Verzeichnis neben dem Binary
#   Windows: scripts\vendor-ffmpeg.ps1 (gleiche gepinnten Assets)
set -euo pipefail

dest="${1:?Aufruf: vendor-ffmpeg.sh <zielverzeichnis>}"
mkdir -p "$dest"

# --- Gepinnte Versionen ------------------------------------------------------
BTBN_TAG="autobuild-2026-10-31-12-59"
BTBN_VER="n9.0.2-39-g7adfbe40dc"   # ffmpeg 9.0.2, LGPL-Variante
FFMPEG_SRC_VER="9.0.2"
FFMPEG_SRC_SHA256="8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e" # ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz

case "$BTBN_VER" in
  n9.0.2-*) BTBN_SHA_LINUX64="6046d6731b26f58887e953561551d254a6eec9fbca4100dfeab56e19af8328d5"
            BTBN_SHA_LINUXARM64="3d1f66827a73ce5a446312bc4924234dc1515726d3743e2df35c0177e1f82128" ;;
esac

os=$(uname -s)
arch=$(uname -m)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

verify_sha256() { # <datei> <sha256>
  local file="$1" want="$2" got
  got=$(shasum -a 256 "$file" | awk '{print $1}')
  [ "$got" = "$want" ] || { echo "SHA-256-Prüfung fehlgeschlagen für $file" >&2
    echo "  erwartet: $want" >&2; echo "  ist:      $got" >&2; exit 1; }
}

check_binary() { # <ffmpeg-pfad> – Version, LGPL-Config, HW-Decoder, Decode-Test
  local ff="$1" out cfg
  out=$("$ff" -version) || { echo "ffmpeg -version fehlgeschlagen: $ff" >&2; exit 1; }
  echo "$out" | head -1 | grep -qE "^ffmpeg version n?${FFMPEG_SRC_VER}" \
    || { echo "Unerwartete ffmpeg-Version: $(echo "$out" | head -1)" >&2; exit 1; }
  cfg=$(echo "$out" | grep "^configuration:" || true)
  if echo "$cfg" | grep -qE -- "--enable-gpl|--enable-nonfree|--enable-libx264|--enable-libx265"; then
    echo "FEHLER: Build enthält GPL-/Nonfree-Komponenten – nicht als LGPL ausliefern!" >&2
    echo "$cfg" >&2; exit 1
  fi
  # Hardware-Beschleuniger müssen gemeldet werden (videotoolbox/vaapi/qsv/d3d11va/…).
  "$ff" -hide_banner -hwaccels 2>/dev/null | grep -qE '^[a-z0-9_]+$' \
    || { echo "Keine Hardware-Beschleuniger in $ff gemeldet (-hwaccels leer)." >&2; exit 1; }
  # Decode-Smoke-Test: nativ encodieren (mpeg4, kein GPL) und mit -hwaccel auto decodieren.
  local t="$work/smoke.avi"
  "$ff" -hide_banner -loglevel error -f lavfi -i testsrc=duration=0.4:size=160x120:rate=10 \
    -c:v mpeg4 -f avi -y "$t" || { echo "Smoke-Test (Encode) fehlgeschlagen." >&2; exit 1; }
  "$ff" -hide_banner -loglevel error -hwaccel auto -i "$t" -f null - \
    || { echo "Smoke-Test (Decode mit -hwaccel auto) fehlgeschlagen." >&2; exit 1; }
  echo "$out" > "$dest/BUILD-ffmpeg.txt"
}

write_license() { # <quelle-beschreibung>
  cat > "$dest/LICENSE-ffmpeg.txt" <<EOF
ffmpeg/ffprobe in diesem Verzeichnis: $1
Lizenz: GNU LGPL v2.1 oder später. Der Build enthält keine GPL- oder
Nonfree-Komponenten (kein --enable-gpl/--enable-nonfree, kein x264/x265/xvid;
wird vor dem Ausliefern per "ffmpeg -version" geprüft, siehe BUILD-ffmpeg.txt).
Quellen: https://github.com/FFmpeg/FFmpeg (Tag n${FFMPEG_SRC_VER})
Lizenztext: https://www.gnu.org/licenses/lgpl-2.1.html
Hinweis: Hardware-Decoder (VideoToolbox/VAAPI/QSV/D3D11VA/CUDA) sind keine
GPL-Komponenten und in diesen LGPL-Builds enthalten.
EOF
}

install_bins() { # <bin-dir>
  install -m 0755 "$1/ffmpeg" "$dest/ffmpeg"
  install -m 0755 "$1/ffprobe" "$dest/ffprobe"
}

case "$os:$arch" in
  Linux:x86_64)
    asset="ffmpeg-${BTBN_VER}-linux64-lgpl-9.0.tar.xz"
    url="https://github.com/BtbN/FFmpeg-Builds/releases/download/${BTBN_TAG}/${asset}"
    echo "Lade ffmpeg LGPL (${asset}) …"
    curl -fL --retry 3 -o "$work/ffmpeg.tar.xz" "$url"
    verify_sha256 "$work/ffmpeg.tar.xz" "$BTBN_SHA_LINUX64"
    tar -xf "$work/ffmpeg.tar.xz" -C "$work"
    bin_dir=$(dirname "$(find "$work" -type f -name ffmpeg -path '*/bin/*' | head -1)")
    install_bins "$bin_dir"
    check_binary "$dest/ffmpeg"
    write_license "statischer LGPL-Build von BtbN/FFmpeg-Builds (${BTBN_TAG}, ${asset}), SHA-256 geprüft"
    ;;
  Linux:aarch64|Linux:arm64)
    asset="ffmpeg-${BTBN_VER}-linuxarm64-lgpl-9.0.tar.xz"
    url="https://github.com/BtbN/FFmpeg-Builds/releases/download/${BTBN_TAG}/${asset}"
    echo "Lade ffmpeg LGPL (${asset}) …"
    curl -fL --retry 3 -o "$work/ffmpeg.tar.xz" "$url"
    verify_sha256 "$work/ffmpeg.tar.xz" "$BTBN_SHA_LINUXARM64"
    tar -xf "$work/ffmpeg.tar.xz" -C "$work"
    bin_dir=$(dirname "$(find "$work" -type f -name ffmpeg -path '*/bin/*' | head -1)")
    install_bins "$bin_dir"
    check_binary "$dest/ffmpeg"
    write_license "statischer LGPL-Build von BtbN/FFmpeg-Builds (${BTBN_TAG}, ${asset}), SHA-256 geprüft"
    ;;
  Darwin:*)
    # macOS: eigener LGPL-Build aus dem offiziellen Quell-Tarball.
    command -v clang >/dev/null || { echo "clang fehlt (Xcode Command Line Tools installieren)." >&2; exit 1; }
    echo "Lade ffmpeg-Quellen ${FFMPEG_SRC_VER} (ffmpeg.org) …"
    curl -fL --retry 3 -o "$work/ffmpeg.tar.xz" "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_SRC_VER}.tar.xz"
    verify_sha256 "$work/ffmpeg.tar.xz" "$FFMPEG_SRC_SHA256"
    tar -xf "$work/ffmpeg.tar.xz" -C "$work"
    src="$work/ffmpeg-${FFMPEG_SRC_VER}"
    extra=""
    if [ "$arch" = "x86_64" ] && ! command -v nasm >/dev/null; then
      extra="--disable-x86asm" # ohne nasm; SIMD-ASM verzichtbar, HW-Decoder bleiben aktiv
      echo "Hinweis: nasm nicht gefunden – baue mit --disable-x86asm." >&2
    fi
    echo "Baue ffmpeg ${FFMPEG_SRC_VER} als LGPL (--disable-gpl --disable-nonfree, ohne x264/x265) …"
    # shellcheck disable=SC2086
    ( cd "$src" && ./configure \
        --disable-gpl --disable-nonfree --disable-libx264 --disable-libx265 \
        --disable-doc --disable-ffplay --disable-debug \
        --enable-static --disable-shared $extra \
        --disable-autodetect --enable-videotoolbox --enable-audiotoolbox --enable-zlib \
        --prefix="$work/prefix" \
      && make -j"$(sysctl -n hw.ncpu 2>/dev/null || echo 4)" \
      && make install )
    install_bins "$work/prefix/bin"
    check_binary "$dest/ffmpeg"
    # Nur Systembibliotheken: Auf Rechnern ohne Homebrew liefe ffmpeg sonst nicht.
    for bin in "$dest/ffmpeg" "$dest/ffprobe"; do
      foreign=$(otool -L "$bin" | tail -n +2 | awk '{print $1}' | grep -vE '^(/usr/lib/|/System/)' || true)
      [ -z "$foreign" ] || { echo "FEHLER: $bin hängt an fremden Bibliotheken:" >&2; echo "$foreign" >&2; exit 1; }
    done
    write_license "eigener LGPL-Build aus ffmpeg-${FFMPEG_SRC_VER}.tar.xz (ffmpeg.org, SHA-256 ${FFMPEG_SRC_SHA256:0:16}…), configure: --disable-gpl --disable-nonfree --disable-libx264 --disable-libx265"
    ;;
  *)
    echo "Nicht unterstützte Plattform: $os/$arch (Windows: scripts/vendor-ffmpeg.ps1 nutzen)." >&2
    exit 1 ;;
esac

echo "OK: ffmpeg + ffprobe (LGPL, geprüft) + LICENSE-ffmpeg.txt + BUILD-ffmpeg.txt nach $dest"
