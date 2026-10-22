# ADR 0014: server MCP locale in sola lettura

- **Stato:** accettata
- **Data:** 2026-10-20

## Contesto

La Fase 6 prevede un server MCP (Model Context Protocol) locale e
facoltativo, che esponga le note alle app di AI dell'utente (Claude Code,
Claude Desktop e altri client MCP). Le regole sono: sola lettura per
impostazione predefinita, permessi minimi, niente server esposto in rete,
registro delle azioni senza contenuto delle note e difese contro la prompt
injection.

## Decisione

- **Trasporto stdio, nessuna porta.** Il client avvia l'eseguibile di
  Greatnote con `--mcp` (più `--data-dir <cartella>`), poi scambia messaggi
  JSON-RPC 2.0 su stdin/stdout, uno per riga. In questa modalità non si apre
  alcuna finestra né si inizializza Tauri, e i log vanno su stderr. Senza
  porte non serve autenticazione di rete: il server è accessibile solo ai
  processi che l'utente stesso configura e avvia.
- **Un solo eseguibile.** La modalità è un argomento del programma
  principale, non un binario separato: i pacchetti `.deb`/AppImage non
  cambiano e la configurazione indica l'eseguibile installato (in un
  AppImage, il file `$APPIMAGE`).
- **Database in sola lettura.** `Database::open_read_only` apre il file con
  `SQLITE_OPEN_READ_ONLY` e `PRAGMA query_only`, senza migrazioni: se lo
  schema non coincide con quello della versione chiede di aprire prima
  l'app. Anche un errore nel codice degli strumenti non può scrivere.
  SQLite gestisce la lettura concorrente con l'app aperta (busy timeout
  5 s).
- **Attivazione esplicita e revoca immediata.** L'impostazione `mcp`
  (`enabled`, predefinito `false`) si cambia solo dall'app, in
  Impostazioni › Integrazioni. Il server la rilegge a ogni chiamata:
  disattivarla blocca subito anche le sessioni già avviate.
- **Strumenti (tutti con `readOnlyHint`):** `search_notes` (stessa sintassi
  della ricerca dell'app, al massimo 50 risultati, query di al massimo 500
  caratteri), `read_note` (metadati e Markdown, testo delle immagini
  incluso, al massimo 100.000 caratteri), `list_notebooks`, `list_tags`.
  Nessuno strumento di modifica in questa versione. Se verranno aggiunti
  saranno separati, disattivati per impostazione predefinita e con
  conferma nell'app per le operazioni distruttive.
- **Privacy.** Le note nel cestino non sono mai restituite. Le note dei
  taccuini esclusi dall'AI (ADR 0010) non compaiono né nelle ricerche né
  in `read_note`, che risponde come per una nota inesistente.
- **Prompt injection.** Le istruzioni del server e il testo di `read_note`
  dichiarano il contenuto come dato dell'utente, non come istruzioni. Gli
  strumenti in sola lettura limitano l'effetto di un'istruzione ostile in
  una nota: non può modificare o cancellare nulla tramite Greatnote.
- **Registro attività.** `mcp-audit.jsonl` nella cartella dati: orario,
  strumento, esito, numero di risultati e, per `read_note`, l'id della
  nota. Mai il testo delle note o delle ricerche. Ruota oltre 1 MB
  (`.1`). Le ultime 20 voci sono visibili nelle impostazioni.
- **Protocollo.** Versioni `2025-06-18`, `2025-03-26` e `2024-11-05`
  (quella del client se supportata, altrimenti la più recente); metodi
  `initialize`, `ping`, `tools/list`, `tools/call`, più elenchi vuoti per
  risorse e prompt. Le richieste in batch sono rifiutate, le righe oltre
  1 MB anche.

## Alternative scartate

- **Server HTTP su localhost:** richiede porta, autenticazione e protezione
  da richieste di altri processi o pagine web. Stdio basta per i client
  desktop.
- **SDK MCP esterno:** il sottoinsieme necessario (JSON-RPC su righe e
  quattro metodi) è piccolo e testato. Una dipendenza asincrona aggiuntiva
  non è giustificata ora; da rivalutare per risorse, notifiche o HTTP.
- **Accesso diretto al file SQLite da parte del client:** nessun controllo
  su esclusioni, cestino e registro.

## Conseguenze

- Ciò che un'app di AI legge tramite MCP può essere inviato al suo
  fornitore: le impostazioni lo dicono esplicitamente all'attivazione.
- Su Windows l'eseguibile usa il sottosistema grafico in release. Le pipe
  stdio ereditate dal client funzionano, ma va verificato nella Fase 8.

## Aggiornamento: strumenti di modifica (2026-10-22)

- Nuovo permesso `allow_write` («Consenti anche le modifiche»), separato
  dall'attivazione e disattivato per impostazione predefinita, riletto a
  ogni chiamata. Senza, `tools/list` non mostra gli strumenti di modifica e
  `tools/call` li rifiuta.
- Solo operazioni **non distruttive**, così non serve una conferma che MCP
  non sa chiedere dentro Greatnote: `create_note` (Markdown, taccuino, tag),
  `append_to_note` (solo in fondo), `create_task`, `complete_task`
  (reversibile; per le ricorrenti nasce la successiva). Nuovo strumento di
  lettura `list_tasks`.
- La connessione in sola lettura resta per tutte le letture. Le modifiche
  usano una seconda connessione aperta alla prima modifica, senza
  migrazioni e solo se lo schema coincide. I taccuini esclusi dall'AI sono
  trattati come inesistenti. Limite di 100 modifiche per avvio del client.
- `append_to_note` registra la nota come modificata da fuori
  (`sync_remote_edits`): se l'app, con la nota aperta, salva dalla versione
  precedente, la versione MCP viene conservata nella cronologia e tra i
  conflitti, come per la sincronizzazione.
- Rimandati: strumenti distruttivi (cestino, riscrittura del testo), che
  richiederebbero una conferma nell'app con una coda di richieste.

