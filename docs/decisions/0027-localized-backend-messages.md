# ADR 0027: messaggi del backend in italiano e inglese

- **Stato:** accettata
- **Data:** 2026-11-14

## Contesto

L'interfaccia è tradotta in `frontend/src/i18n.ts`, ma i messaggi d'errore
prodotti in Rust (circa 250 tra `#[error(...)]` di `thiserror` e stringhe
restituite dai comandi Tauri) erano solo in italiano e arrivavano così come
sono nell'interfaccia: con il sistema in inglese l'app era in inglese e gli
errori in italiano. In un punto (`sync_commands.rs`) il testo dell'errore
veniva anche confrontato per riconoscere un token Dropbox scaduto.

## Opzioni valutate

1. **Codici d'errore tradotti nel frontend**: ogni comando restituisce
   `{ code, params }` e `i18n.ts` traduce. Pulito, ma richiede di cambiare
   il tipo di ritorno di tutti i comandi (`Result<T, String>`), di
   serializzare errori annidati (I/O, SQLite, HTTP) e di aggiornare i 134
   punti del frontend che mostrano errori.
2. **Messaggi localizzati in Rust**: il nucleo conosce la lingua
   dell'interfaccia e formatta i messaggi in quella lingua.

## Decisione

Opzione 2, con il modulo `greatnote_core::l10n`:

- `tr!("italiano {}", "English {}", arg)` restituisce `String` e sceglie la
  lingua quando il messaggio viene formattato, quindi funziona anche dentro
  `#[error("{}", tr!(..., .0))]`. Gli argomenti espliciti sono
  posizionali; le variabili catturate nel testo (`{error}`) funzionano come
  in `format!`.
- `tr_str!("italiano", "English")` restituisce `&'static str` senza
  formattazione: va bene dove serviva il letterale (campi `&'static str`,
  `Result<_, &'static str>`, argomenti `&str`). Nessuna delle due funziona
  in un `const`: lì si usa una closure o una funzione.
- La lingua è una variabile globale: all'avvio dall'ambiente
  (`LC_ALL`/`LC_MESSAGES`/`LANG`, inglese se assenti come su Windows e
  Android), poi l'interfaccia la conferma con il comando esistente
  `set_ui_language`, ora chiamato su tutte le piattaforme.
- Se nessuno la imposta vale l'italiano, così i test non dipendono dalla
  lingua della macchina; `l10n::with_language` la cambia solo per il thread
  corrente nei test.
- Restano in italiano i log (`tracing`), destinati allo sviluppo, e i
  testi che non sono messaggi: SQL, nomi di file e cartelle delle
  esportazioni, i modelli di nota (già bilingui), i prompt e i testi per i
  modelli AI e per i client MCP (`ai/prompt.rs`, `ai/planner_context.rs`,
  `mcp.rs`), che gestiscono la lingua per conto loro.
- Il codice non riconosce più gli errori dal testo: il token Dropbox
  scaduto è `FolderError::Unauthorized` (`SyncError::is_unauthorized`).

## Conseguenze

- Il contratto dei comandi Tauri resta `Result<T, String>`: nessuna modifica
  al frontend.
- Le traduzioni del backend stanno accanto al messaggio, non in
  `i18n.ts`: una terza lingua richiederebbe di estendere `tr!` o di passare
  all'opzione 1.
- I messaggi già salvati (es. ultimo errore di sincronizzazione) restano
  nella lingua in cui sono stati prodotti.
