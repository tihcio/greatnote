# Architettura

## Obiettivi

- Applicazione desktop local-first, utilizzabile senza account o rete.
- Dominio e persistenza Rust indipendenti dalla shell desktop.
- Frontend React + TypeScript con bridge Tauri ristretto a comandi applicativi
  tipizzati.
- UI e logica non legate a KDE, GNOME o a un altro desktop environment.
- Dati locali sotto il controllo dell'utente; nessun servizio esterno
  necessario.

## Struttura iniziale

```text
.
├── src/                    # core Rust: dominio, storage, ricerca,
│                           # conversione contenuti, backup, import/export
├── migrations/             # migrazioni SQLite versionate
├── src-tauri/              # shell desktop Tauri 2 e comandi
├── frontend/               # UI TypeScript/React e configurazione Vite
├── docs/decisions/         # ADR
└── docs/                   # guide, sicurezza e test
```

Il core Rust non dipende da Tauri. Il frontend non apre il database: comunica
con la shell tramite comandi Tauri. Le API native specifiche della piattaforma
si aggiungono solo quando una funzionalità le richiede e dietro un'interfaccia
portabile.

## Confini e dati

- **Dominio:** note, spazi, taccuini, tag e validazione del documento
  strutturato.
- **Storage:** SQLite locale, migrazioni versionate e repository Rust con
  transazioni per relazioni e operazioni di organizzazione.
- **Shell:** inizializzazione del percorso dati e comandi Tauri piccoli e
  validati.
- **UI:** layout responsive, localizzazione e stato di salvataggio; nessuna
  dipendenza da API di KDE.

Le note usano UUID e timestamp UTC. Il contenuto è JSON strutturato, versionato
e validato in Rust; non si usa HTML arbitrario come fonte di verità. Il
database è collocato nella directory dati dell'app fornita da Tauri, quindi
segue le convenzioni della piattaforma (XDG su Linux). La shell mantiene le
decorazioni standard della finestra, lasciando gestione e integrazione al
window manager attivo.

Spazi e taccuini hanno ordinamento persistito e i tag sono univoci senza
distinzione tra maiuscole e minuscole. Una nota appartiene al massimo a un
taccuino e può avere più tag. Eliminare uno spazio rimuove solo il contenitore:
i taccuini restano senza spazio. Eliminare un taccuino scollega le note invece
di cancellarle. Eliminare un tag rimuove le sue associazioni in transazione.
Le note nel cestino sono conservate con `deleted_at` e possono essere
ripristinate.

## Persistenza iniziale

SQLite è utilizzato tramite `rusqlite` con SQLite bundled, evitando di
dipendere dalla versione SQLite di sistema. Le migrazioni sono file SQL
versionati: `0001_initial.sql` crea il modello base,
`0002_note_drafts.sql` aggiunge le bozze recuperabili e
`0003_search_productivity.sql` introduce l'indice FTS5, visite recenti,
allegati e collegamenti; `0004` completa il testo indicizzato degli allegati
e `0005_settings_and_index_repair.sql` aggiunge le impostazioni, la
provenienza delle note importate e reindicizza le note escluse dalla vecchia
ricostruzione FTS. Prima di aggiornare uno schema esistente, l'apertura del
database crea un backup completo `premigration`. L'indice FTS5 è aggiornato nella stessa operazione
del salvataggio della nota; gli allegati aggiornano il testo indicizzato
tramite trigger. Le query utente vengono analizzate in filtri tipizzati e i
valori sono sempre passati come parametri SQLite; soltanto frammenti SQL
costanti selezionano le viste.

Prima di aggiornare la nota, l'autosave conserva la bozza in una tabella
separata. Al riavvio, quando si apre quella nota, l'utente può ripristinare o
scartare la bozza; il contenuto salvato in precedenza non viene sovrascritto
prima della scelta. L'app elimina la bozza solo dopo aver applicato il
contenuto alla nota o dopo una richiesta esplicita di scarto.

## Confini desktop e sicurezza

- Nessun accesso generico a filesystem o shell esposto al frontend.
- I comandi Rust sono l'unico confine di persistenza e validano gli input.
- CSP locale restrittiva nel bundle. In sviluppo è prevista una CSP separata,
  che consente lo script inline React Refresh e il WebSocket HMR solo sul
  server Vite loopback `127.0.0.1:1420`.
- Nessuna origine remota è necessaria.
- File e dati utente non sono inseriti nei log.
- Le capability Tauri concedono soltanto i permessi core necessari.

## Ricerca, allegati e collegamenti

Il parser in `src/search.rs` limita le query a 512 caratteri e 32
termini/filtri, supporta frasi e filtri per tag/taccuino/spazio/date/allegati/
preferite/archivio, ma non supporta ancora operatori booleani. Input
malformati producono un errore leggibile. I risultati sono paginati nel core e
includono anteprime testuali evidenziate; filtri e query FTS non sono mai
concatenati dal testo utente.

Gli allegati sono conservati fuori da SQLite in `attachments/`, con nomi UUID
generati dall'app. Sono accettati file di testo, PNG, JPEG, GIF e PDF fino a
25 MiB; MIME, estensione, dimensioni e SHA-256 vengono controllati prima del
salvataggio/apertura. I file di testo sono indicizzati fino a 100.000
caratteri. L'apertura passa dal plugin Tauri opener solo dopo aver verificato
percorso e hash. I collegamenti sono relazioni locali persistite in
`note_links`, distinte dal documento rich-text.

## Import/export e protezione dati

Moduli del core:

- `src/content.rs`: conversioni documento ⇄ Markdown/HTML/testo. L'import
  costruisce soltanto nodi ammessi (nessun HTML conservato) e limita
  annidamento e schemi dei link.
- `src/transfer.rs`: export di note, ZIP Markdown e archivio `gna-archive` (identificatore stabile);
  import con piano/anteprima e commit in un'unica transazione.
- `src/backup.rs`: backup ZIP verificabili, ripristino con backup di
  sicurezza e rollback, backup automatici, controllo d'integrità e
  ricostruzione dell'indice; copie cifrate con password in formato age
  ([ADR 0006](./docs/decisions/0006-encrypted-backups-and-rename.md)).

I comandi corrispondenti sono in `src-tauri/src/data_commands.rs`: aprono i
dialog nativi dal lato Rust, eseguono il lavoro su thread bloccanti e non
accettano percorsi dal frontend (i backup interni sono indicati per nome
convalidato). Il PDF usa il dialog di stampa di sistema. Formati documentati
in [docs/ARCHIVE_FORMAT.md](./docs/ARCHIVE_FORMAT.md); motivazioni in
[ADR 0005](./docs/decisions/0005-import-export-and-backups.md).

## Attività, promemoria e calendario

- `src/time.rs`: conversioni fra orari locali, istanti UTC e date civili con
  `jiff`; strategia "compatible" per l'ora legale.
- `src/planner.rs`: repository di attività, eventi e promemoria, viste
  calcolate su una data "oggi" passata dal chiamante, presa in carico
  atomica dei promemoria scaduti e campi facoltativi dell'archivio completo.
- `src-tauri/src/planner_commands.rs`: comandi tipizzati e scheduler (thread
  dedicato, controllo ogni 15 s) che invia notifiche con
  `tauri-plugin-notification` ed emette `reminders-fired` verso la UI.
- Frontend: `TasksView`, `RemindersView`, `CalendarView`, `NotePlanner` e
  `ReminderAlerts` in `frontend/src/features/`; le viste sostituiscono lista
  ed editor nell'area principale (stato `workspace`).

Motivazioni in [ADR 0007](./docs/decisions/0007-tasks-reminders-calendar.md).

## Cestino

`src/trash.rs` gestisce il cestino di spazi, taccuini, tag, attività ed
eventi; le note hanno il proprio flusso in `storage.rs`. Spostando un
contenitore nel cestino i collegamenti vengono staccati e registrati in
`trash_relations`, così il resto dell'app non vede riferimenti a elementi
cestinati; il ripristino li ricrea. Motivazioni in
[ADR 0008](./docs/decisions/0008-trash-for-all-items.md).

## Menu e interfaccia

- `frontend/src/app/buildMenus.ts`: modello unico dei menu (barra, menu
  contestuale, scorciatoie, elenco scorciatoie); `app/menu.ts` gestisce
  acceleratori e ricerca dell'azione; `app/preferences.ts` le preferenze.
- `frontend/src/editor/`: estensioni Tiptap, barra degli strumenti, comandi
  dei blocchi, trova e sostituisci (con decorazioni), menu "/" e
  normalizzazione del documento prima del salvataggio.
- `src/library.rs`: revisioni, modelli, copia di note, file, contatori,
  informazioni e preferenze; `src-tauri/src/ui_commands.rs`: comandi
  corrispondenti, finestra (zoom, schermo intero, tema, finestra nota) e
  appunti.

Motivazioni in [ADR 0009](./docs/decisions/0009-desktop-menus-and-full-interface.md).

## Importazione da altre app

- `src/external_import/`: `enex` (Evernote, XML in streaming), `joplin`
  (archivio JEX), `obsidian` (vault). Ognuno produce un `ImportPlan` con
  taccuino d'origine, chiavi e collegamenti per nota; `mod.rs` contiene le
  verifiche comuni e la riscrittura di link e incorporamenti nel Markdown
  (fuori dal codice).
- `src/transfer.rs`: destinazione `keep-structure` (taccuini e spazio
  creati nella stessa transazione) e collegamenti salvati in `note_links`.

Motivazioni in [ADR 0011](./docs/decisions/0011-import-from-evernote-joplin-obsidian.md).

## Sincronizzazione

- `src/sync/` (ADR 0017):
  - `tables`: tabelle sincronizzate, istantanee, applicazione con
    riferimenti;
  - `folder`: cartella condivisa, lotti, blob, scritture atomiche;
  - `mod`: identità, esportazione con coda di uscita, importazione causale,
    conflitti, allegati in attesa.
- Trasporti dietro il trait `SyncStore` (`folder.rs`): `LocalStore`
  (cartella) e `DropboxStore` (`dropbox.rs`, API v2 con PKCE, ADR 0018). Il
  desktop sceglie il trasporto dalle impostazioni e conserva il token
  Dropbox nel portachiavi.
- Migrazione 14: tabelle `sync_*` e tre trigger per ogni tabella
  sincronizzata, che annotano le modifiche in `sync_dirty` (disattivati
  durante l'applicazione di modifiche remote).
- `Database::update_note_from` riceve la versione su cui lavorava l'editor
  e conserva un testo remoto non visto.
- `src-tauri/src/sync_commands.rs`: impostazioni, scelta della cartella,
  esecuzione manuale e ogni 2 minuti, evento `sync-finished` per
  l'interfaccia.
- `frontend/src/features/SyncSection.tsx`: sezione delle impostazioni.
  `App.tsx` gestisce l'indicatore di stato e ricarica la nota aperta.

## Calendari iCal

- `src/calendar_sync/ics.rs`: lettura iCalendar ed espansione delle
  ricorrenze con `jiff`, inclusi i fusi Windows.
- `src/calendar_sync/subscriptions.rs`: abbonamenti (tabella
  `calendar_subscriptions`, colonna `events.subscription_id`, migrazione
  13), download sicuro, aggiornamento della copia locale (ADR 0016).
- `calendar_commands.rs` conserva l'indirizzo nel portachiavi e include gli
  abbonamenti nella sincronizzazione periodica.
- `IcsSubscriptions.tsx` è la sezione delle impostazioni, sempre visibile.

## Calendari esterni

- `src/calendar_sync/`:
  - `oauth` (PKCE, URL, scambio e aggiornamento dei token, revoca, lettura
    del ritorno);
  - `google` e `microsoft` (trait `CalendarService` sulle API REST);
  - `http` (client senza reindirizzamenti, codifica);
  - `mod` (account e calendari in SQLite, copia degli eventi, sincronizzazione
    e modifiche con ETag).

  Migrazione 12: tabelle `calendar_accounts`, `external_calendars` e colonne
  esterne di `events` (ADR 0015).
- `src-tauri/src/calendar_commands.rs`:
  - accesso nel browser con listener di loopback;
  - token nel portachiavi e in memoria;
  - sincronizzazione ogni 15 minuti con evento `calendar-sync-finished`;
  - comandi per eventi esterni.
- `frontend/src/features/calendar/`: API, sezione delle impostazioni. Colori
  e sola lettura in `CalendarView` ed `EventDialog`.

## Server MCP locale

- `src/mcp.rs`: server MCP in sola lettura (JSON-RPC 2.0 su righe, stdio),
  strumenti `search_notes`, `read_note`, `list_notebooks` e `list_tags`,
  impostazione `mcp` e registro `mcp-audit.jsonl` senza contenuti (ADR 0014).
  `Database::open_read_only` apre il file senza migrazioni e con
  `query_only`.
- `src-tauri/src/mcp_commands.rs`: modalità `greatnote --mcp [--data-dir]`
  avviata prima di Tauri (log su stderr), più i comandi `get_mcp_status` e
  `set_mcp_enabled` per Impostazioni › Integrazioni.

## AI facoltativa

- `src/ai/`: `settings` (impostazioni, indirizzo verificato, consenso),
  `provider` (trait `AiProvider` e client compatibile OpenAI con streaming,
  annullamento ed errori tipizzati), `chunking` (frammenti con posizioni in
  caratteri), `index` (vettori in SQLite, ricerca per coseno,
  indicizzazione incrementale), `retrieval` (fonti semantiche o FTS5,
  rilettura dalle note correnti), `planner_context` (attività, eventi e
  promemoria pertinenti alla domanda, con date riconosciute localmente;
  ADR 0022), `prompt` (messaggi con delimitatori non
  falsificabili, risposte strutturate).
- `src/ai/builtin/`: motore integrato (ADR 0020).
  - `catalog`: modelli fissati per commit e SHA-256.
  - `store`: download con ripresa e verifica.
  - `server`: processi `llama-server` su 127.0.0.1 con chiave casuale,
    avviati su richiesta e fermati dopo l'inattività.
  - `BuiltinProvider`: implementa `AiProvider` inoltrando le richieste a
    `OpenAiCompatible`.

  I comandi sono in `src-tauri/src/builtin_ai_commands.rs`.
- `src-tauri/src/ai_commands.rs`: comandi asincroni con identificativo per
  l'annullamento, streaming tramite `Channel`, chiave API nel portachiavi
  (`keyring`), indicizzazione in background con eventi
  `ai-index-progress`/`ai-index-finished`.
- `frontend/src/features/ai/`: vista Assistente (chat e ricerca per
  significato), dialog di anteprima con confronto, suggerimenti di tag e
  attività, sezione delle impostazioni; voci nel menu Strumenti e nella barra
  dell'editor.
- Il frontend non contatta mai i server AI (la CSP lo impedisce): tutto il
  traffico parte dal processo Rust.

Motivazioni in [ADR 0010](./docs/decisions/0010-optional-ai-providers-and-semantic-search.md).

## Limiti conosciuti

La sincronizzazione non è ancora implementata; l'AI copre la Fase 5 (vedi
ADR 0010 per ciò che resta escluso).
I promemoria scattano solo con l'app aperta. I backup interni non sono cifrati (solo le copie esportate possono esserlo). La verifica visiva e end-to-end nella
finestra desktop resta distinta dai test di compilazione.
