# ADR 0002: storage locale SQLite

- **Stato:** accettata
- **Data:** 2026-10-13

## Contesto

Le note devono funzionare offline, rimanere locali e poter essere ricercate e
salvate in modo affidabile.

## Decisione

Usare SQLite con `rusqlite` e SQLite bundled. Versionare le migrazioni SQL;
conservare i documenti editoriali come JSON strutturato con versione esplicita.
Le operazioni di database sono esposte tramite il crate core Rust, non
direttamente al frontend.

## Conseguenze

- I test del core non richiedono una GUI o le librerie GTK.
- La versione SQLite inclusa rende la disponibilità delle funzionalità meno
  dipendente dal sistema; FTS5 va verificato con test prima di usarlo.
- La migrazione v2 aggiunge il recupero delle bozze e viene testata da uno
  schema v1 contenente già note.
- Backup, restore e backup preventivo pre-migrazione rimangono da
  implementare.
