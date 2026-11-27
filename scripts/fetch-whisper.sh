#!/usr/bin/env bash
# Scarica whisper.cpp per la trascrizione delle riunioni su Linux (ADR 0036)
# e lo prepara in src-tauri/resources/whisper, da cui i pacchetti
# .deb/.rpm/AppImage lo copiano accanto all'app. Gemello di
# scripts/fetch-whisper.ps1. La cartella è separata da quella di llama.cpp:
# le due build usano versioni diverse delle librerie ggml. Versione e hash
# sono fissati: un archivio diverso da quello previsto viene rifiutato.
# Idempotente.
#
# Uso (dalla radice del repository; lo eseguono già da sé `tauri build` e
# `tauri dev` su Linux):
#   bash scripts/fetch-whisper.sh
set -euo pipefail

BUILD="b5907"
ARCHIVE="whisper-bin-ubuntu-x64.tar.gz"
SHA256="a6199027fe5b4239c84171d2bca4c0ff44ec86a515494d5a39b237b212d429bd"
ARCHIVE_URL="https://github.com/ggml-org/whisper.cpp/releases/download/$BUILD/$ARCHIVE"
LICENSE_URL="https://raw.githubusercontent.com/ggml-org/whisper.cpp/$BUILD/LICENSE"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="$root/src-tauri/resources/whisper"
marker="$target/VERSION.txt"
cache="$root/target/whisper-dist"

if [[ -f "$marker" && "$(tr -d '[:space:]' < "$marker")" == "$BUILD" \
      && -x "$target/whisper-cli" ]]; then
  echo "whisper.cpp $BUILD presente in $target"
  exit 0
fi
if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "fetch-whisper: solo x86_64 è supportato" >&2
  exit 1
fi

mkdir -p "$cache"
archive="$cache/whisper-$BUILD-$ARCHIVE"
if [[ ! -f "$archive" ]] || ! echo "$SHA256  $archive" | sha256sum --check --status; then
  echo "Scarico $ARCHIVE_URL"
  curl --fail --location --proto '=https' --tlsv1.2 --retry 3 \
    --output "$archive.part" "$ARCHIVE_URL"
  mv "$archive.part" "$archive"
fi
if ! echo "$SHA256  $archive" | sha256sum --check --status; then
  rm -f "$archive"
  echo "fetch-whisper: hash dell'archivio diverso dal previsto, file rimosso" >&2
  exit 1
fi

work="$(mktemp -d "$cache/extract.XXXXXX")"
trap 'rm -rf "$work"' EXIT
tar -xzf "$archive" -C "$work" --no-same-owner
source_dir="$(find "$work" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
output="$work/out"
mkdir "$output"

# Solo whisper-cli e le librerie che carica: server, test e strumenti di
# Parakeet non servono all'app.
cp "$source_dir/whisper-cli" "$output/"
for library in "$source_dir"/*.so*; do
  name="$(basename "$library")"
  case "$name" in
    libwhisper.so* | libggml.so* | libggml-base.so* | libggml-cpu-*.so)
      [[ -L "$library" ]] || cp "$library" "$output/"
      ;;
  esac
done
# Ogni libreria prende il nome del suo SONAME: nessun link simbolico nel
# pacchetto, nessun doppione.
while IFS= read -r -d '' library; do
  soname="$(LC_ALL=C readelf -d "$library" 2>/dev/null | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')"
  if [[ -n "$soname" && "$soname" != "$(basename "$library")" ]]; then
    mv "$library" "$output/$soname"
  fi
done < <(find "$output" -type f -name '*.so*' -print0)
chmod 644 "$output"/*.so*
chmod 755 "$output/whisper-cli"
if ! LD_LIBRARY_PATH="" ldd "$output/whisper-cli" | grep -q "libwhisper.so.1 => $output"; then
  echo "fetch-whisper: whisper-cli non trova le sue librerie" >&2
  exit 1
fi

curl --fail --location --proto '=https' --tlsv1.2 --retry 3 \
  --output "$output/LICENSE-whisper.cpp.txt" "$LICENSE_URL"
echo "$BUILD" > "$output/VERSION.txt"

rm -rf "$target"
mkdir -p "$(dirname "$target")"
mv "$output" "$target"
echo "whisper.cpp $BUILD pronto in $target ($(du -sh "$target" | cut -f1))"
