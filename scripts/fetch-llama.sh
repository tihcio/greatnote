#!/usr/bin/env bash
# Scarica il motore llama.cpp distribuito con Greatnote su Linux (ADR 0020,
# ADR 0024) e lo prepara in src-tauri/resources/llama, da cui i pacchetti
# .deb/.rpm/AppImage lo copiano accanto all'app. Gemello di
# scripts/fetch-llama.ps1: stessa build, con Vulkan (GPU NVIDIA, AMD e Intel,
# altrimenti CPU). Versione e hash sono fissati: un archivio diverso da
# quello previsto viene rifiutato. Idempotente.
#
# Uso (dalla radice del repository, prima di `npx tauri build` su Linux; lo
# eseguono già da sé `tauri build` e `tauri dev` su Linux):
#   bash scripts/fetch-llama.sh
set -euo pipefail

BUILD="b11397"
ARCHIVE="llama-$BUILD-bin-ubuntu-vulkan-x64.tar.gz"
SHA256="4604dbcacb4bb3aff7571cdd839ddf7d09231179c44f34cea115337bb506add9"
ARCHIVE_URL="https://github.com/ggml-org/llama.cpp/releases/download/$BUILD/$ARCHIVE"
LICENSE_URL="https://raw.githubusercontent.com/ggml-org/llama.cpp/$BUILD/LICENSE"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="$root/src-tauri/resources/llama"
marker="$target/VERSION.txt"
cache="$root/target/llama-dist"

if [[ -f "$marker" && "$(tr -d '[:space:]' < "$marker")" == "$BUILD" \
      && -x "$target/llama-server" ]]; then
  echo "llama.cpp $BUILD presente in $target"
  exit 0
fi
if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "fetch-llama: solo x86_64 è supportato" >&2
  exit 1
fi

mkdir -p "$cache"
archive="$cache/$ARCHIVE"
if [[ ! -f "$archive" ]] || ! echo "$SHA256  $archive" | sha256sum --check --status; then
  echo "Scarico $ARCHIVE_URL"
  curl --fail --location --proto '=https' --tlsv1.2 --retry 3 \
    --output "$archive.part" "$ARCHIVE_URL"
  mv "$archive.part" "$archive"
fi
if ! echo "$SHA256  $archive" | sha256sum --check --status; then
  rm -f "$archive"
  echo "fetch-llama: hash dell'archivio diverso dal previsto, file rimosso" >&2
  exit 1
fi

work="$(mktemp -d "$cache/extract.XXXXXX")"
trap 'rm -rf "$work"' EXIT
tar -xzf "$archive" -C "$work" --no-same-owner
source_dir="$(find "$work" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
output="$work/out"
mkdir "$output"

# Solo il server e le librerie che carica: gli altri strumenti a riga di
# comando (e le loro librerie *-impl) non servono all'app.
cp "$source_dir/llama-server" "$output/"
for library in "$source_dir"/*.so*; do
  name="$(basename "$library")"
  case "$name" in
    libllama-server-impl.so | libllama.so* | libllama-common.so* | libmtmd.so* \
      | libggml.so* | libggml-base.so* | libggml-cpu-*.so | libggml-vulkan.so)
      # -L: i link simbolici diventano file; sotto si tiene un solo nome.
      [[ -L "$library" ]] || cp "$library" "$output/"
      ;;
  esac
done
# Ogni libreria prende il nome del suo SONAME (quello cercato dal linker,
# es. libllama.so.0): nessun link simbolico nel pacchetto, nessun doppione.
while IFS= read -r -d '' library; do
  soname="$(LC_ALL=C readelf -d "$library" 2>/dev/null | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')"
  if [[ -n "$soname" && "$soname" != "$(basename "$library")" ]]; then
    mv "$library" "$output/$soname"
  fi
done < <(find "$output" -type f -name '*.so*' -print0)
chmod 644 "$output"/*.so*
chmod 755 "$output/llama-server"
if ! LD_LIBRARY_PATH="" ldd "$output/llama-server" | grep -q "libllama-server-impl.so => $output"; then
  echo "fetch-llama: llama-server non trova le sue librerie" >&2
  exit 1
fi

curl --fail --location --proto '=https' --tlsv1.2 --retry 3 \
  --output "$output/LICENSE-llama.cpp.txt" "$LICENSE_URL"
echo "$BUILD" > "$output/VERSION.txt"

rm -rf "$target"
mkdir -p "$(dirname "$target")"
mv "$output" "$target"
echo "llama.cpp $BUILD pronto in $target ($(du -sh "$target" | cut -f1))"
