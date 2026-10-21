#!/usr/bin/env bash
# Controlli automatici della checklist di rilascio (docs/RELEASE_CHECKLIST.md).
# Funziona su Linux e su Windows con Git Bash. Si ferma al primo errore.
#
# Uso: scripts/release-check.sh [--ui]
#   --ui  esegue anche il test dell'interfaccia (richiede Firefox).
set -euo pipefail

cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$1"; }

step "Formattazione Rust"
cargo fmt --all --check

step "Clippy (workspace, tutti i target)"
cargo clippy --workspace --all-targets --all-features -- -D warnings

step "Test Rust (workspace)"
cargo test --workspace

step "Audit dipendenze Rust (RustSec)"
if command -v cargo-audit >/dev/null 2>&1; then
    cargo audit
else
    echo "cargo-audit non installato: cargo install cargo-audit --locked" >&2
    exit 1
fi

step "Dipendenze frontend"
npm ci

step "Build frontend"
npm run build

step "Audit dipendenze npm"
npm audit

if [[ "${1:-}" == "--ui" ]]; then
    step "Test interfaccia"
    npm run test:ui
fi

step "Nessun segreto OAuth nel repository"
if git grep -nE 'GOCSPX-[A-Za-z0-9_-]{10,}|sk-ant-[A-Za-z0-9_-]{10,}|sk-[A-Za-z0-9]{32,}' -- . ':!*.lock' ':!package-lock.json'; then
    echo "Possibili segreti trovati: rimuoverli prima del rilascio" >&2
    exit 1
fi

printf '\nControlli automatici superati. Completa i passi manuali in docs/RELEASE_CHECKLIST.md.\n'
