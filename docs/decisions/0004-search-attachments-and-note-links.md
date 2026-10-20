# ADR 0004: ricerca locale, allegati e collegamenti tra note

- **Stato:** accettata
- **Data:** 2026-10-14

## Contesto

La fase di produttività deve rimanere offline, non esporre SQL o filesystem
generici alla UI e preservare le note già esistenti durante l'evoluzione
dello schema.

## Decisione

- Usare FTS5 bundled per titolo, testo strutturato e contenuto testuale degli
  allegati. Aggiornare l'indice nella stessa transazione delle modifiche e
  con trigger per gli allegati.
- Analizzare le query in filtri tipizzati con limiti espliciti e parametri
  SQLite. Supportare testo, frasi e filtri documentati; rifiutare operatori
  booleani/parentesi finché non è disponibile un parser completo.
- Conservare gli allegati come file locali con nome UUID, dimensione massima
  25 MiB, tipi verificati, hash SHA-256 e controllo del file/percorso prima
  dell'apertura. Conservare nel database solo metadati e testo ricercabile.
- Conservare i collegamenti interni come relazione `note_links`, separata dal
  documento Tiptap, così la navigazione resta indipendente dal formato editor.
- Registrare l'apertura di una nota in `note_visits` per alimentare la vista
  Recenti.

## Conseguenze

- La migrazione v3 costruisce l'indice delle note esistenti senza sostituire
  il database.
- L'indice dei file di testo è limitato ai primi 100.000 caratteri; i file
  binari sono cercabili per nome e per filtro di presenza.
- L'apertura usa il plugin Tauri opener soltanto dopo la verifica d'integrità;
  il frontend non riceve accesso arbitrario ai percorsi.
- Gli operatori booleani non sono supportati e generano un errore. Backup,
  esportazione e pulizia definitiva degli allegati sono fasi successive.
