#!/usr/bin/env bash
# Aggiornamenti in-app per i pacchetti Linux (ADR 0023, ADR 0024). Gemello
# di scripts/updater-manifest.ps1:
#
# - verifica che la versione del tag coincida con quella dell'app;
# - firma AppImage, .deb e .rpm con la chiave degli aggiornamenti e la
#   versione vincolata (campo "version:" del commento firmato, richiesto da
#   requireSignedVersion) e controlla che la firma la contenga;
# - aggiunge a latest.json le voci linux-x86_64-appimage, -deb e -rpm, con
#   l'URL del pacchetto nella stessa release.
#
# Richiede TAURI_SIGNING_PRIVATE_KEY e TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# nell'ambiente (segreti del repository). Uso:
#   scripts/updater-manifest-linux.sh v0.2.0 tihcio/greatnote \
#     target/release/linux-packages latest.json
set -euo pipefail

tag="$1"
repository="$2"
packages="$3"
manifest="$4"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$root/src-tauri/tauri.conf.json")"
if [[ "$tag" != "v$version" ]]; then
  echo "Il tag $tag non corrisponde alla versione dell'app ($version): aggiorna la versione prima del tag." >&2
  exit 1
fi
if [[ ! -f "$manifest" ]]; then
  echo "Manifest $manifest assente: deve crearlo prima il job Windows." >&2
  exit 1
fi

# Versione nel commento firmato (riga "trusted comment:" della firma).
signed_version() {
  base64 --decode < "$1" | sed -n 's/^trusted comment: *//p' | tr '\t' '\n' \
    | sed -n 's/^version://p' | head -n 1 | tr -d '[:space:]'
}

declare -A files=(
  [appimage]="Greatnote.x86_64.AppImage"
  [deb]="Greatnote.amd64.deb"
  [rpm]="Greatnote.x86_64.rpm"
)
entries=()
for kind in appimage deb rpm; do
  file="$packages/${files[$kind]}"
  if [[ ! -f "$file" ]]; then
    echo "Pacchetto mancante: $file" >&2
    exit 1
  fi
  rm -f "$file.sig"
  npx tauri signer sign --app-version "$version" "$file" > /dev/null
  if [[ "$(signed_version "$file.sig")" != "$version" ]]; then
    echo "La firma di $file non contiene la versione $version." >&2
    exit 1
  fi
  entries+=("$kind" "$(tr -d '\n' < "$file.sig")" \
    "https://github.com/$repository/releases/download/$tag/${files[$kind]}")
done

python3 - "$manifest" "$version" "${entries[@]}" <<'PY'
import json, sys
path, version, *rest = sys.argv[1:]
with open(path, encoding="utf-8") as handle:
    manifest = json.load(handle)
if manifest.get("version") != version:
    sys.exit(f"latest.json indica la versione {manifest.get('version')}, attesa {version}")
platforms = manifest.setdefault("platforms", {})
for index in range(0, len(rest), 3):
    kind, signature, url = rest[index:index + 3]
    platforms[f"linux-x86_64-{kind}"] = {"signature": signature, "url": url}
with open(path, "w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2, ensure_ascii=False)
    handle.write("\n")
print(f"Aggiunte a {path}: " + ", ".join(sorted(k for k in platforms if k.startswith("linux-"))))
PY
