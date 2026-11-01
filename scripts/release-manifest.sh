#!/usr/bin/env bash
# U4/U5: signiertes Update-Manifest (latest.json + latest.json.sig) aus den
# fertigen Paketen in einem Verzeichnis erzeugen.
#
#   scripts/release-manifest.sh <paketverzeichnis> <version> [release-notes-file]
#
# Signatur: Ed25519 (64-Byte-Detached-Signatur) mit `cargo packager signer sign`,
# privater Schlüssel aus $CARGO_PACKAGER_SIGN_PRIVATE_KEY (CI-Secret, nie im
# Repo). Ohne Schlüssel wird das Manifest NICHT signiert – das Paket ist dann
# ein klar markiertes Test-Paket und die App installiert es nicht.
set -euo pipefail

dir="${1:?paketverzeichnis}"
version="${2:?version}"
notes_file="${3:-}"

cd "$dir"

# Git Bash auf Windows hat sha256sum, macOS nur shasum.
sha256_of() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

assets=$(find . -maxdepth 1 -type f \( -name '*.exe' -o -name '*.dmg' -o -name '*.deb' -o -name '*.zip' -o -name '*.AppImage' \) | sed 's|^\./||' | sort)
[ -n "$assets" ] || { echo "keine Pakete in $dir gefunden" >&2; exit 1; }

notes=""
[ -n "$notes_file" ] && [ -f "$notes_file" ] && notes=$(cat "$notes_file")

# latest.json aufbauen (Assets mit URL + SHA-256; URL zeigt auf den Release-Download).
tag="${GITHUB_REF_NAME:-v$version}"
base="https://github.com/${GITHUB_REPOSITORY:-stckl/kuhplay}/releases/download/$tag"
{
  printf '{"version":"%s","notes":%s,"assets":[' "$version" "$(python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' <<<"$notes")"
  first=1
  for a in $assets; do
    sha=$(sha256_of "$a")
    [ $first = 1 ] || printf ','
    first=0
    printf '{"name":"%s","url":"%s/%s","sha256":"%s"}' "$a" "$base" "$a" "$sha"
  done
  printf ']}\n'
} > latest.json

if [ -n "${CARGO_PACKAGER_SIGN_PRIVATE_KEY:-}" ]; then
  cargo packager signer sign latest.json -k "$CARGO_PACKAGER_SIGN_PRIVATE_KEY"  # schreibt latest.json.sig
  echo "OK: latest.json signiert (Ed25519) -> latest.json.sig"
else
  echo "WARNUNG: CARGO_PACKAGER_SIGN_PRIVATE_KEY fehlt – latest.json bleibt UNSIGNIERT (Test-Paket, Installation in der App gesperrt)." >&2
  : > latest.json.sig
fi
echo "Manifest: $dir/latest.json"
