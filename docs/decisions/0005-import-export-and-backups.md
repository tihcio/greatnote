# ADR 0005: import/export in formati aperti, backup e ripristino

- **Stato:** accettata
- **Data:** 2026-10-14

## Contesto

La fase 3 richiede di importare ed esportare senza lock-in, proteggere i dati
con backup verificabili e offrire una procedura di recupero, senza esporre al
frontend API generiche di filesystem e senza perdere dati in caso di errore.

## Decisione

- **Conversione del contenuto** in `src/content.rs`: il documento Tiptap v1
  viene convertito in Markdown (CommonMark + front matter con metadati in JSON),
  HTML autonomo e testo. L'importazione usa `pulldown-cmark` per il Markdown e
  `scraper` (html5ever) per l'HTML, ma costruisce **solo** nodi e contrassegni
  ammessi dallo schema; l'HTML non viene mai conservato. Link ammessi solo con
  `http`, `https` e `mailto`. L'annidamento è limitato per restare valido.
- **Esportazione**: nota singola (`.md`, `.html`, `.txt`), ZIP Markdown di un
  taccuino o di tutte le note con allegati in cartelle relative, archivio
  completo `gna-archive` v1 (JSON + allegati, vedi
  [ARCHIVE_FORMAT](../ARCHIVE_FORMAT.md)). I file vengono scritti in un file
  temporaneo e rinominati solo a scrittura completata.
- **PDF**: generato dal motore della webview con il foglio di stile di stampa
  dedicato, invece di includere un motore PDF e font nel binario. *Aggiornato
  in 0.2.12:* il file si sceglie con il selettore nativo e viene scritto senza
  dialog di stampa: su Linux una `WebKitPrintOperation` stampa sulla
  stampante virtuale «Stampa su file» di GTK (cercata con il nome tradotto;
  se manca l'operazione fallisce senza ripiegare su una stampante reale), su
  Windows `ICoreWebView2_7::PrintToPdf`. Il file è scritto con nome
  temporaneo nella cartella di destinazione, controllato (`%PDF-`) e
  rinominato. La stampa su carta resta `WebviewWindow::print` (`Ctrl+P`).
- **Importazione in due fasi**: un piano analizza i file e produce
  un'anteprima (dry-run) con nuove note, duplicati ed errori; il commit applica
  le voci nuove in un'**unica transazione SQLite**. Gli allegati scritti prima
  di un errore vengono rimossi. Per i file la destinazione predefinita è un
  nuovo taccuino "Importazione AAAA-MM-GG". I duplicati sono riconosciuti per
  hash di titolo+testo (file) o per ID (archivio).
- **Dialog nativi lato Rust**: i comandi aprono `tauri-plugin-dialog` dal
  processo Rust; il frontend non riceve permessi del plugin né percorsi, e il
  piano in attesa è identificato da un token monouso.
- **Backup**: ZIP con `manifest.json`, copia coerente del database tramite
  `VACUUM INTO` e allegati, ciascuno con dimensione e SHA-256. Tipi:
  manuale, automatico (all'avvio, ogni N giorni, conservazione configurabile),
  pre-migrazione (creato automaticamente prima di aggiornare uno schema
  esistente) e pre-ripristino.
- **Ripristino**: verifica completa (manifest, hash, `PRAGMA
  integrity_check`, allegati referenziati), migrazione della copia, backup di
  sicurezza dello stato corrente, poi scambio dei file con rollback in caso di
  errore. La connessione viene riaperta senza riavviare l'app.
- **Integrità**: `PRAGMA integrity_check`, `foreign_key_check`, controllo
  interno FTS5, note non indicizzate e hash di ogni allegato; ricostruzione
  dell'indice su richiesta.

## Conseguenze

- Nuove dipendenze del core: `pulldown-cmark` (MIT), `scraper` (ISC), `zip`
  (MIT, solo deflate); nella shell `tauri-plugin-dialog` (MIT/Apache-2.0).
- I backup **non sono cifrati**: sono protetti da permessi 0600/0700 nella
  cartella dati. La cifratura richiede un modello di chiavi e recupero e resta
  aperta.
- La migrazione v5 aggiunge `settings`, `notes.source` e reindicizza le note
  escluse dalla ricostruzione FTS v3→v4.
- Il formato ENEX resta escluso fino alla fase 6.
