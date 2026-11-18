# Porting Android

Stato: **fasi A–E fatte, F in parte, G avviata.** L'app gira su un Pixel 7
Pro (Android 17) con disposizione per telefono: note, editor, ricerca,
allegati, backup e ripristino, import ed export, promemoria come notifiche
di sistema, sincronizzazione Dropbox, calendari iCal, condivisione verso
altre app. L'AI con server compatibile OpenAI è visibile ma nessuna risposta
è stata provata. La build di rilascio (APK e AAB) si compila, si firma con
una chiave fuori dal repository e parte sul telefono; non è stata
pubblicata. Ultimo aggiornamento: 2026-11-16 (Greatnote 0.2.5).

Questo documento descrive cosa serve per portare Greatnote su Android
riusando il codice esistente. Ogni affermazione è marcata come *verificata*
(letta nel codice o nella documentazione della versione in uso) oppure come
*ipotesi* da confermare con una build reale. Gli esiti delle prove sono nel
registro al §10.

## 1. Architettura attuale

Verificato sul repository:

| Parte | Dove | Note |
|---|---|---|
| Dominio, storage, ricerca, sync, import/export, backup, AI | crate `greatnote-core` (radice, `src/`, ~31.000 righe) | Nessuna dipendenza da Tauri o dalla GUI |
| Shell e comandi Tauri | crate `greatnote-desktop` (`src-tauri/`, ~6.900 righe) | Tauri 2.12.3, wry 0.57, tao 0.37 |
| Interfaccia | `frontend/` — React 19, TypeScript, Vite 8, Tiptap 3 | `App.tsx` 4.017 righe, `styles.css` 6.850 righe |
| Schema | `migrations/0001…0015` | SQLite incluso nella build (`rusqlite` con `bundled`) |

Punti favorevoli al porting:

- La separazione core/shell esiste già: il core non conosce Tauri.
- `src-tauri/Cargo.toml` dichiara già `crate-type = ["staticlib", "cdylib", "rlib"]`,
  richiesto dalle build mobili.
- Il frontend parla con Rust solo tramite `invoke` ed eventi: nessun plugin
  JavaScript di Tauri, nessun accesso diretto al filesystem.
- Gli allegati passano già come base64 attraverso un comando
  (`add_note_attachment`), non come percorsi.
- La cartella dati viene da `app.path().app_data_dir()`, non da percorsi fissi.
- TLS con rustls: nessuna dipendenza da OpenSSL di sistema.

Punti che richiedono lavoro:

- `run()` in `src-tauri/src/lib.rs` non ha il punto d'ingresso mobile
  (`#[cfg_attr(mobile, tauri::mobile_entry_point)]`) e restituisce un
  `Result`; registra inoltre plugin e servizi solo desktop senza distinzione.
- La finestra ha `minWidth: 780`; il CSS ha soglie a 1000, 780 e 560 px ma è
  pensato per finestre desktop strette, non per l'uso touch.
- Tutti i dialog di file usano le varianti bloccanti e convertono il
  risultato in un percorso (`FilePath::into_path`).

## 2. Dipendenze e plugin

### Plugin Tauri (versioni dal `Cargo.lock`)

| Plugin | Versione | Android | Fonte |
|---|---|---|---|
| `tauri-plugin-dialog` | 2.8.2 | Sì, con limiti: **nessun selettore di cartelle**; restituisce **content URI**, non percorsi | Documentazione del plugin (verificato) |
| `tauri-plugin-opener` | 2.7.1 | Solo apertura di URL; `open_path` non è supportato su Android (verificato nel sorgente), quindi gli allegati si aprono con `FilesPlugin.kt` | Documentazione e sorgente del plugin |
| `tauri-plugin-fs` | 2.7.0 | Solo su Android, solo dal lato Rust: apre i content URI in lettura e scrittura. Nessun permesso al frontend | Provato sull'emulatore |
| `tauri-plugin-clipboard-manager` | 2.4.2 | Elencato tra le piattaforme supportate; livello da verificare con una build | Da verificare |
| `tauri-plugin-notification` | 2.6.0 | Supportato; richiede permesso e canali. Notifiche programmate ad app chiusa: da verificare | Documentazione del plugin (parziale) |
| `tauri-plugin-updater` | 2.14.0 | **Solo desktop**: la documentazione lo installa e lo registra soltanto per macOS/Windows/Linux | Documentazione del plugin (verificato) |

### Crate nativi

| Crate | Uso | Android |
|---|---|---|
| `rusqlite` 0.42 `bundled` | Database, FTS5 | Compila con l'NDK e il database si apre sull'emulatore (verificato) |
| `android-native-keyring-store` 1.0 + `keyring-core` 1.0 | Segreti su Android (MIT/Apache-2.0, stessa organizzazione di `keyring`) | Provato sul Pixel |
| `keyring` 4.2 (feature `v1`) | Chiavi API AI, token Dropbox e calendari sul desktop | La feature `v1` copre «Mac, Windows e \*nix». Esiste uno store `android-native-keyring-store` opzionale, non attivo nel progetto. Il crate compila per Android; **da verificare** se a runtime c'è uno store utilizzabile |
| `notify-rust` 4.18 | Notifiche promemoria su Linux | Dipendenza attivata da `cfg(all(unix, not(target_os = "macos")))`: la condizione era vera anche su Android: ora lo esclude |
| `ureq` 3.4 con rustls | Provider AI, Dropbox, calendari | Rust puro. Origine dei certificati radice su Android: da verificare |
| `dirs` 7 | Cartella dati per `greatnote --mcp` | Usato solo dal server MCP, escluso su Android |
| `age`, `image`, `zip`, `tar`, `quick-xml`, `scraper`, `pulldown-cmark`, `jiff`, `sha2`, `uuid` | Backup cifrati, immagini, archivi, import, date | Rust puro, nessun ostacolo noto. `jiff` su Android: origine del database dei fusi orari da verificare |

### Codice legato al sistema operativo

Verificato con una ricerca nel codice:

| Punto | Cosa fa | Su Android |
|---|---|---|
| `src/ai/builtin/server.rs`, `hardware.rs` | Avvia `llama-server` come processo figlio su porta locale | Non portabile così com'è |
| `src/ai/claude_code.rs` | Avvia la CLI `claude` cercandola nel `PATH` | Non disponibile |
| `src/ocr.rs` | Avvia `tesseract` cercandolo nel `PATH` | Non disponibile |
| `src-tauri/src/update_commands.rs` | Aggiornamento in-app, `pkexec` per i pacchetti rpm | Non applicabile |
| `src-tauri/src/mcp_commands.rs` | Server MCP su standard input/output (`greatnote --mcp`) | Non applicabile |
| `src-tauri/src/ui_commands.rs` (`open_note_window`) | Seconda finestra per una nota | Android ha una sola webview |
| `src-tauri/src/calendar_commands.rs`, `sync_commands.rs` | OAuth con reindirizzamento su socket locale | Da ripensare (vedi §5) |
| `src/sync/mod.rs` (`default_device_name`) | Legge `HOSTNAME` o `/etc/hostname` | Ricade su «Computer»: serve un nome sensato |
| `src/sync/folder.rs` | Sincronizzazione su cartella scelta dall'utente | Dipende dal selettore di cartelle, assente |
| Permessi `0o700` in `backup.rs`, `storage.rs`, `lib.rs` | Cartelle private | Funziona (`cfg(unix)`), ridondante nella sandbox dell'app |

## 3. Inventario delle funzionalità

Stati: **C** condivisibile, **A** adattabile, **D** solo desktop,
**V** da verificare, **N** non supportata allo stato attuale.

| Funzionalità | Stato | Note |
|---|---|---|
| Note: creazione, modifica, duplicazione, eliminazione | C | Logica nel core; interfaccia da adattare |
| Bozze e recupero dopo chiusura inattesa | C | Va collegato agli eventi di sospensione di Android |
| Recenti, preferiti, archivio, cestino | C | |
| Spazi, taccuini, tag | C | Trascinamento per l'ordinamento: da rifare per il touch (A) |
| Collegamenti tra note | C | |
| Editor Tiptap e formattazione | A | Barra strumenti, menu contestuale, menu «/», selezione e tastiera virtuale |
| Testo cifrato nelle note | C | |
| Cronologia revisioni, modelli | C | |
| Ricerca full-text e filtri | C | FTS5 nella build SQLite inclusa |
| Sostituzione rapida (quick switcher), scorciatoie | A | Le scorciatoie da tastiera hanno poco senso; serve un accesso touch |
| Barra dei menu desktop | D | Sostituita dalla navigazione mobile |
| Nota in finestra separata | D | |
| Allegati: aggiunta | A | Già via base64; il selettore passa per content URI |
| Allegati: apertura con app esterna | V | `open_path` non documentato su Android; probabilmente serve un `FileProvider` |
| Immagini nelle note, annotazioni | A | Annotatore da provare col touch |
| Testo dalle immagini (OCR con tesseract) | N | Processo esterno assente |
| Attività, attività ricorrenti | C | |
| Promemoria | A | Lo scheduler attuale è un thread dell'app: ad app chiusa non scatta. Servono allarmi di sistema |
| Calendario locale (mese, settimana, griglia oraria) | A | Layout da ripensare per schermi stretti |
| Sottoscrizioni iCal | C | Solo HTTP; aggiornamento in background limitato |
| Calendari Google/Microsoft | V | Già sospesi sul desktop; OAuth da rifare |
| Esportazione Markdown, HTML, testo, ZIP | A | Scrittura tramite content URI invece di percorsi |
| Esportazione PDF | V | Da capire come è prodotta e se la webview Android la consente |
| Importazione Markdown, testo, HTML, archivio dell'app | A | Lettura tramite content URI |
| Importazione ENEX, JEX | A | Come sopra |
| Importazione cartelle Markdown, vault Obsidian | N | Richiedono il selettore di cartelle |
| Backup manuale, cifrato, verifica integrità, ripristino | A | Destinazione e origine tramite content URI |
| Backup automatico | A | Nella cartella dell'app funziona; la cartella non è visibile all'utente |
| Sincronizzazione su cartella | N | Nessun selettore di cartelle; accesso a cartelle esterne limitato |
| Sincronizzazione Dropbox | V | Trasporto HTTP riusabile; autorizzazione e portachiavi da rifare |
| AI con provider remoti (API compatibili OpenAI) | V | HTTP riusabile; dipende dal portachiavi |
| AI con server locale in rete (Ollama su un altro computer) | V | Come sopra; richiede traffico in chiaro verso la LAN |
| Motore AI integrato (llama.cpp), ricerca modelli | N | Processo figlio e binari desktop |
| Provider Claude Code | N | CLI desktop |
| Ricerca semantica | V | L'indice è nel core; dipende da un provider di embedding raggiungibile |
| Server MCP locale | D | |
| Aggiornamenti dall'app | A | APK firmato allegato alla release e installatore di sistema (fase G) |
| Impostazioni, temi chiaro/scuro, lingua | C | |
| Credenziali nel portachiavi | V | Vedi `keyring` |

## 4. Compatibilità dei dati

- Lo schema e le migrazioni sono nel core e verranno eseguiti identici:
  **non serve alcuna modifica allo schema** per la prima versione.
- Il nome del database (`gna.sqlite3`) e la struttura `attachments/`,
  `backups/` restano gli stessi, quindi un backup desktop è ripristinabile su
  Android e viceversa (*ipotesi* fondata sul codice condiviso, da provare).
- La migrazione dalla cartella del vecchio identificatore (`dev.gna.notes`)
  su Android non trova nulla e non fa nulla.
- Con la sincronizzazione su cartella non disponibile, nella prima versione
  il passaggio dei dati tra desktop e Android avviene con backup/ripristino o
  con l'archivio di esportazione. Dropbox è la via per una sincronizzazione
  vera e va affrontato come fase a sé.

## 5. Rischi

1. **File e content URI.** È il lavoro più esteso: backup, ripristino,
   import ed export oggi ricevono percorsi. Il core lavora su `Path`;
   la soluzione meno invasiva è copiare il contenuto scelto dall'utente in un
   file temporaneo nella cartella dell'app, passare quel percorso al core e,
   per le esportazioni, scrivere in un file temporaneo e poi copiarlo nella
   destinazione. Richiede di aggiungere `tauri-plugin-fs` (2.7.0 è già nel
   lockfile come dipendenza indiretta) o un piccolo plugin Kotlin.
2. **Sospensione dell'app.** Android può terminare il processo in
   background. Il salvataggio automatico e le bozze devono essere scaricati
   quando l'app va in pausa, non solo alla chiusura della finestra.
3. **Promemoria.** Senza allarmi di sistema i promemoria scattano solo ad
   app aperta. Va dichiarato nell'interfaccia finché non è risolto.
4. **Portachiavi.** Se lo store Android di `keyring` non è utilizzabile,
   le funzioni che richiedono segreti (AI remota, Dropbox) restano
   disattivate: i segreti non vanno mai salvati in chiaro come ripiego.
5. **OAuth.** Il reindirizzamento su socket locale è fragile su Android;
   la via corretta è un collegamento all'app (deep link).
6. **Editor.** Tiptap nella webview Android con tastiera virtuale è il
   rischio principale per l'usabilità: va provato su dispositivo reale presto.
7. **Dimensione di `App.tsx`.** Il layout mobile richiede di separare
   navigazione e contenuto in un file da 4.000 righe; il rischio è introdurre
   regressioni sul desktop. Vanno mantenuti verdi i test UI esistenti.
8. **Toolchain.** Rust è installato dai pacchetti Fedora, senza `rustup`
   (vedi §8).

## 6. Piano di lavoro

Ogni fase lascia Linux e Windows compilabili e con i test verdi.

### Fase A — Ambiente e prima build (nessuna funzione nuova)

- Installare l'ambiente (§8) e generare il progetto con `tauri android init`.
- Punto d'ingresso mobile in `run()`.
- Separare con `cfg(desktop)` / `cfg(mobile)`: updater, server MCP, motore AI
  integrato, Claude Code, OCR, finestra separata, `notify-rust`.
- File `tauri.android.conf.json` e capability dedicata.
- Il frontend riceve dal backend l'elenco delle funzioni disponibili e
  nasconde le altre: nessun pulsante senza comportamento.

Accettazione: l'app si avvia sull'emulatore, apre il database e mostra
l'elenco delle note; `cargo test` e la build Linux invariati.

Fatto: ambiente, progetto generato, punto d'ingresso mobile, separazione di
updater, modalità `--mcp`, `notify-rust`, selettore di cartelle, stampa e
controlli della finestra; capability Android. L'app si avvia e crea il
database.

Le voci non utilizzabili su mobile sono nascoste nell'interfaccia (vedi
fase B): l'elenco è deciso nel frontend (`app/platform.ts`), non esposto dal
backend, perché dipende solo dalla piattaforma.

### Fase B — Interfaccia mobile per note ed editor

- Navigazione a una colonna (elenco → nota), pannello laterale a scomparsa.
- Aree sicure, tasto Indietro, rotazione.
- Barra dell'editor adatta al touch, comportamento con la tastiera virtuale.
- Salvataggio alla sospensione e ripristino dello stato.

Accettazione: creare, modificare, cercare e organizzare note su un telefono
reale; i dati sopravvivono a sospensione e chiusura forzata.

Fatto e provato sull'emulatore:

- `frontend/src/mobile.css`: tutte le regole dipendono da
  `data-platform="mobile"`, quindi il desktop non cambia. Le regole desktop
  per finestre strette (barra laterale a icone) sono escluse su mobile.
- Una colonna alla volta: barra superiore, navigazione a scomparsa, elenco,
  nota. Dialog a tutto schermo.
- Aree sicure con `env(safe-area-inset-*)`; la tastiera riduce l'area
  visibile grazie al margine applicato in `MainActivity.kt`.
- Tasto Indietro (`onBackButtonPress`): chiude dialog e menu, poi la
  navigazione, poi la nota (salvandola), poi torna alla Home; dalla Home
  salva ed esce.
- Salvataggio quando l'app va in background (`visibilitychange`).
- Nascosti su mobile: assistente AI e relative impostazioni, integrazioni
  (MCP), sincronizzazione, aggiornamenti, apertura in finestra o scheda,
  stampa e PDF, import da cartella e da Obsidian. Le impostazioni lo
  dichiarano con un avviso.

Dopo la prima prova su telefono (Pixel 7 Pro, 2026-11-13):

- Barra della formattazione in fondo allo schermo, sopra la tastiera, su
  una riga che scorre; pulsanti da 44px; ordine dei comandi pensato per la
  scrittura. Con la tastiera aperta la riga di tag e stato si nasconde
  (`data-keyboard="open"`, dedotto dall'altezza della finestra).
- Tutti i menu (barra, contestuali, sottomenu) sono pannelli dal basso con
  voci alte 48px e sfondo che chiude al tocco. Prima il menu «Inserisci»
  restava tagliato dalla barra scorrevole.
- Pressione prolungata sul testo: l'app non apre più un proprio menu
  nell'editor e nei campi, così funziona la selezione di sistema con
  maniglie, copia e incolla. Il menu contestuale non si chiude più al
  rilascio del dito.
- Barra laterale: righe da 48px e un pulsante «⋯» per riga che apre le
  azioni (su desktop compaiono al passaggio del mouse).
- Gli errori sono visibili anche dall'elenco e dalle altre viste.
- Tag e stato del salvataggio stanno nel testo che scorre, sotto il
  contenuto, e non in una barra fissa: restano raggiungibili con la
  tastiera aperta. (Una prima versione nascondeva la barra a tastiera
  aperta: toccando il campo dei tag la tastiera si apriva, la barra spariva
  e il campo perdeva il focus.)
- Campi data e ora: la data occupa una riga intera, ora e minuti accanto.

Aggiunti dopo (2026-11-16), **da provare sul telefono**:

- «Incolla senza formattazione», «Testo cifrato» e «Trova e sostituisci»,
  che sul desktop stanno nel menu contestuale dell'editor, su mobile sono in
  fondo al menu «Altro» della barra della formattazione.
- Dopo una chiusura forzata l'app riapre la nota che era aperta. L'id della
  nota è ricordato sul dispositivo (`localStorage`, solo come comodità: se
  manca si riparte dalla Home); una nota finita nel cestino nel frattempo
  non viene riaperta. Una bozza non salvata passa dalla solita richiesta di
  recupero.

Aree di tocco (2026-11-16), dopo la segnalazione di pulsanti troppo
piccoli (per esempio Impostazioni):

- `tests/ui/mobile-targets.mjs` misura i controlli nelle viste principali:
  ne trovava 82 sotto i 44×44 px (pulsanti di testo da 40px, icone della
  nota da 42px, strumenti dell'elenco da 32px, «+» del calendario da 18px,
  caselle e scelte delle impostazioni alte 18px, voci di menu da 42px,
  colori dei calendari da 22px, pulsanti delle impostazioni da 32px). Ora
  nessuno.
- Impostazioni in fondo alla navigazione: icona e nome invece del solo
  simbolo ⚙ nell'angolo.
- Icone nei pulsanti portate a 22px.
- Caselle e scelte: si tocca l'intera riga con l'etichetta.
- Calendario: nel mese e nella settimana il bersaglio è il giorno intero,
  che apre la vista del giorno; lì eventi e «Nuovo evento» sono grandi. Gli
  eventi in miniatura nelle celle non si toccano più singolarmente
  (`TimeGrid.tsx`, classe `overview`). Su telefoni più stretti di 412dp
  l'intestazione del giorno nella settimana scende sotto i 44px di
  larghezza; la colonna sotto resta toccabile.
- Da provare sul telefono.

Limiti noti:

- Ricerca, taccuini, tag, attività e calendario usano le viste desktop
  ridimensionate: si aprono, ma non sono state provate a fondo col tocco.
- Menu contestuali con pressione prolungata, selezione del testo,
  copia/incolla e trascinamento per riordinare: non provati.
- Backup, import, export e allegati sono visibili ma **non provati**: usano
  ancora percorsi di file e sono l'oggetto della fase C.
- Il carattere Manrope dei titoli non compare nelle schermate
  dell'emulatore: da verificare.

### Fase C — File

- Allegati (aggiunta, apertura, rimozione).
- Backup e ripristino, anche cifrati.
- Import ed export dei formati a file singolo.

Accettazione: un backup creato su Linux si ripristina su Android e viceversa,
senza differenze nel contenuto.

Fatto e provato sull'emulatore:

- `src-tauri/src/file_access.rs` è l'unico punto che conosce la differenza
  tra percorsi e content URI. Su Android copia il documento scelto in
  `<cache>/transfer/<area>` (con il plugin `fs`, usato solo dal lato Rust) e
  passa la copia al core; per i salvataggi il core scrive nella cartella
  temporanea e il risultato viene copiato nel documento di destinazione. Il
  core non è stato modificato.
- Le copie temporanee sono rimosse alla selezione successiva, a fine
  operazione, all'annullamento e all'avvio.
- Su Android il selettore non filtra per estensione (`.enex`, `.jex`, `.age`
  non hanno un tipo MIME): si può scegliere qualsiasi file e il controllo
  avviene sul contenuto.
- Apertura degli allegati: `FilesPlugin.kt` (nel progetto Android dell'app)
  apre una copia in `<cache>/open/` tramite `FileProvider`, concedendo
  all'altra app la sola lettura di quel file.
- Aggiunta di allegati: usa il selettore della webview, senza modifiche.
- `android:allowBackup="false"`: il backup automatico di Android non copia
  le note nell'account Google.

Limiti noti:

- Se un'esportazione fallisce dopo la scelta della destinazione, il
  documento che Android crea al momento della scelta viene eliminato
  (`FilesPlugin.deleteDocument`, solo URI di documenti), così non resta un
  file vuoto. Aggiunto il 2026-11-16, **non provato** con un errore reale.
- Il testo d'aiuto dell'importazione su mobile non cita più la cartella di
  Obsidian e rimanda alla versione per computer.
- ENEX, JEX, archivio dell'app, esportazione ZIP e copia cifrata usano lo
  stesso percorso di codice delle prove eseguite, ma non sono stati provati
  uno per uno.
- File grandi (centinaia di MB): non provati.

### Fase D — Attività, promemoria, calendario

- Viste adattate; notifiche con permesso e canale; allarmi di sistema.

Fatto e provato su telefono (promemoria):

- `src-tauri/src/reminder_alarms.rs`: su Android ogni promemoria
  programmato è affidato al sistema come notifica a orario (plugin delle
  notifiche, `AlarmManager`). Il database resta la fonte di verità; la
  funzione `sync` riallinea le notifiche a ogni modifica e a ogni giro dello
  scheduler, e non fa nulla se l'insieme non è cambiato.
- Lo scheduler dell'app su Android aggiorna solo lo stato dei promemoria:
  non manda una seconda notifica e non segnala come «persi» quelli già
  consegnati dal sistema.
- Canale «Promemoria» (importanza alta). Permesso `POST_NOTIFICATIONS`
  chiesto al primo promemoria o dalla vista Promemoria, che avvisa se manca.
- `cancel_all` del plugin 2.6.0 fallisce su Android (argomenti nulli): si
  annullano le notifiche in attesa una per una.

Limiti noti:

- **Ritardo.** Senza il permesso per gli allarmi esatti (che l'app non
  chiede) Android consegna entro una finestra: nella prova 1 minuto e 47
  secondi dopo l'orario. In risparmio energetico può essere di più.
- Al massimo 100 promemoria futuri affidati al sistema per volta; gli altri
  entrano man mano che i primi scattano e l'app viene aperta.
- La notifica non ha azioni (posticipa, fatto): toccarla apre l'app.
  L'icona piccola è un foglio stilizzato (`ic_stat_notify`) al posto
  dell'icona generica di sistema. Il plugin 2.6.0 non accetta un'icona
  predefinita nella configurazione di Tauri (la sezione
  `plugins.notification` fa fallire l'avvio: provato), quindi l'icona è
  indicata su ogni notifica in `reminder_alarms.rs`.
- **Telefoni che congelano le app (verificato su vivo X300 Pro, Android
  16, 2026-11-18).** Chiudendo Greatnote dalle app recenti il sistema la
  congela e rimuove le sue notifiche programmate (registro degli allarmi:
  `Removal history … Reason=frozen`); il promemoria non arriva. Con
  Greatnote esente dall'ottimizzazione della batteria lo stesso promemoria
  è arrivato puntuale. La vista Promemoria e il pannello promemoria della
  nota mostrano un avviso con **Consenti in background**, che apre la
  richiesta di sistema (`REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`). Sul Pixel
  7 Pro non serve.
- **Permesso delle notifiche.** Era chiesto solo aggiungendo un promemoria
  dal pannello della nota: non per le attività né per i promemoria arrivati
  dalla sincronizzazione, che quindi non comparivano. Ora lo chiedono anche
  il dialog dell'attività e l'avvio dell'app, se ci sono promemoria
  programmati e il permesso non è mai stato deciso.
- Dopo un riavvio del telefono le notifiche sono ripristinate dal plugin:
  **non provato**.
- Chiusura dalle app recenti e arresto forzato dalle impostazioni: non
  provati (l'arresto forzato cancella gli allarmi per regola di Android).
- Attività e calendario: controlli portati a 44px, aggiunta rapida su più
  righe, intestazione della settimana leggibile, avviso dei promemoria
  ridotto a una riga che porta alla vista Promemoria. Viste guardate nelle
  schermate del telefono; creazione e modifica di attività ed eventi, e
  trascinamento sulla griglia oraria, **non provati a mano**.

### Fase E — Sincronizzazione e integrazioni

- Portachiavi; Dropbox; iCal; condivisione da e verso altre app.

Fatto:

- **Portachiavi.** `src-tauri/src/secrets.rs` è l'unico punto che legge e
  scrive segreti. Su Android l'API `v1` di `keyring` restituisce un errore
  (verificato nel sorgente: nessuno store per la piattaforma), quindi si usa
  `keyring-core` con `android-native-keyring-store` 1.0: preferenze cifrate
  con una chiave dell'Android Keystore. Provato sul Pixel con una voce di
  prova: scrittura, lettura ed eliminazione riuscite. Nessun ripiego su
  file in chiaro.
- **Dropbox.** La sezione Sincronizzazione è visibile su mobile con il solo
  trasporto Dropbox. Il codice di accesso è lo stesso del desktop (browser
  di sistema, PKCE, ritorno su `http://localhost:41927/`). La chiave
  dell'app entra nella build da `GREATNOTE_DROPBOX_APP_KEY`, come sul
  desktop.
- **Calendari iCal.** La sezione Integrazioni su mobile mostra le sole
  sottoscrizioni iCal; server MCP e account Google/Microsoft restano
  desktop.
- **Condivisione.** «Condividi con un'altra app…» nel menu della nota apre
  il pannello di Android con il testo della nota (`FilesPlugin.shareText`).
  Limite di 100.000 caratteri; oltre, l'app invita a esportare un file.

Limiti noti:

- Accesso a Dropbox e prima sincronizzazione provati sul Pixel: le note
  degli altri dispositivi sono arrivate. Il ritorno dal browser all'app è
  manuale, e funziona solo se Android non ha terminato l'app nel frattempo.
- La prima sincronizzazione ha mostrato 10 «modifiche contemporanee» che
  non riguardavano il telefono: era la storia dei conflitti tra gli altri
  dispositivi, riapplicata. Difetto del motore di sincronizzazione (non di
  Android), corretto in `src/sync/mod.rs` con un test; vale per ogni
  dispositivo che entra dopo gli altri.
- Durante la stessa sincronizzazione un allegato remoto è stato scartato
  per il vincolo di unicità (stessa nota, stesso contenuto, due righe):
  il file è comunque presente. Da indagare a parte.
- La sincronizzazione gira solo mentre l'app è aperta (ogni 2 minuti): non
  c'è lavoro in background.
- iCal: aggiunta di un calendario e aggiornamento **non provati** (prima
  richiesta HTTPS dal lato Rust su Android).
- Ricevere contenuti condivisi da altre app (testo o file verso Greatnote):
  non implementato.
- La condivisione invia solo testo, senza allegati.

### Fase F — AI

- Provider remoti e server in rete locale; ricerca semantica.
- Valutazione separata di un motore locale su Android.

Fatto:

- Su mobile sono di nuovo visibili le impostazioni AI, l'assistente e i
  comandi AI dell'editor, con il solo provider compatibile con OpenAI. Il
  codice è quello del desktop: richieste HTTP dal lato Rust, chiave API nel
  portachiavi (Android Keystore, già provato), consenso per i server non
  locali.
- La scelta del provider è nascosta. Impostazioni arrivate da un backup del
  computer con il motore integrato o Claude Code vengono riportate al
  provider compatibile con OpenAI quando si salva.
- Sul telefono «localhost» è il telefono stesso: le impostazioni lo dicono
  e suggeriscono l'indirizzo del computer in rete. Un indirizzo di rete
  locale è trattato come server remoto, con conferma degli invii.

Non fatto:

- **Nessuna risposta AI provata su Android**: sul computer di sviluppo non
  c'era un server in esecuzione. Connessione, chat, modifica AI e ricerca
  per significato sono da provare con un server raggiungibile dal telefono.
- Motore locale su Android (llama.cpp): non valutato.
- Lettura del testo dalle immagini: disponibile solo tramite un modello con
  visione; tesseract non esiste su Android.

### Fase G — Distribuzione

- APK firmato per i test, AAB, `versionCode`, permessi motivati,
  informativa sulla privacy, voce nella checklist di rilascio.

Fatto (2026-11-16):

- **Identità del pacchetto.** `dev.greatnote.notes`, nome «Greatnote».
  `versionName` e `versionCode` vengono da `version` in
  `src-tauri/tauri.conf.json` (Tauri li scrive in `app/tauri.properties`):
  0.2.5 → `versionCode` 2005 (major·1.000.000 + minor·1.000 + patch).
  `minSdk` 24 (Android 7.0), `targetSdk` e `compileSdk` 37.
- **Firma.** `app/build.gradle.kts` firma il rilascio solo se trova i dati
  della chiave, in `src-tauri/gen/android/keystore.properties` (ignorato da
  git) oppure nelle variabili d'ambiente `GREATNOTE_ANDROID_KEYSTORE`,
  `GREATNOTE_ANDROID_KEYSTORE_PASSWORD`, `GREATNOTE_ANDROID_KEY_ALIAS`,
  `GREATNOTE_ANDROID_KEY_PASSWORD`. Senza, il rilascio esce non firmato
  (`app-universal-release-unsigned.apk`). Nessuna chiave è stata creata:
  va decisa e custodita dall'editore (vedi sotto).
- **Permessi** del pacchetto finale (`aapt2 dump permissions`):
  `INTERNET` (Dropbox, iCal, AI), `POST_NOTIFICATIONS` (promemoria),
  `RECEIVE_BOOT_COMPLETED` e `WAKE_LOCK` (il plugin delle notifiche
  ripristina i promemoria dopo il riavvio e li consegna col telefono in
  attesa), `REQUEST_INSTALL_PACKAGES` (aggiornamenti dall'app, dalla
  0.2.10), `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` (richiesta di esenzione
  per i promemoria ad app chiusa), più il permesso interno
  `DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION` aggiunto da AndroidX. Nessun
  accesso a file, posizione, contatti, fotocamera o microfono: i file
  passano dal selettore di sistema.
- **Solo telefono e tablet.** Tolto il supporto Android TV generato da
  Tauri (`leanback`): l'interfaccia non è pensata per il telecomando.
- **Dimensioni.** Il modello di Tauri teneva i simboli della libreria nativa
  anche nel rilascio (il blocco `packaging` dentro il tipo `debug` vale per
  tutto `android`). Ora valgono solo per il debug e Gradle usa l'NDK di
  `NDK_HOME` per ripulire la libreria: APK arm64 da 35 a 24 MB. L'AAB
  contiene in più la tabella dei simboli per Play (`BUNDLE-METADATA`), che
  non arriva ai telefoni.
- **R8.** La build di rilascio riduce codice e risorse. Le classi Kotlin
  chiamate per riflessione sono elencate in `app/proguard-rules.pro`;
  l'icona delle notifiche, cercata per nome, è protetta da
  `res/raw/keep.xml` (senza, R8 la eliminava: verificato).

Comandi:

```sh
# APK di rilascio per telefoni arm64 e AAB per Play
npx tauri android build --target aarch64 --apk --aab
# APK: src-tauri/gen/android/app/build/outputs/apk/universal/release/
# AAB: src-tauri/gen/android/app/build/outputs/bundle/universalRelease/

# Creazione della chiave di firma (una volta, fuori dal repository)
keytool -genkeypair -v -keystore ~/greatnote-release.jks -alias greatnote \
  -keyalg RSA -keysize 4096 -validity 10000

# src-tauri/gen/android/keystore.properties (non versionato)
storeFile=/percorso/assoluto/greatnote-release.jks
storePassword=…
keyAlias=greatnote
keyPassword=…
```

La chiave è definitiva: tutte le versioni future dello stesso APK devono
essere firmate con la stessa, altrimenti Android rifiuta l'aggiornamento.
Va conservata con una copia di sicurezza. Con Google Play, la firma per la
distribuzione può essere affidata a Play (Play App Signing) e la chiave
locale diventa la chiave di caricamento.

Per provare il rilascio senza la chiave definitiva: firmare l'APK con la
chiave di debug locale (`zipalign` + `apksigner` dei Build-Tools con
`~/.android/debug.keystore`). Si installa sopra una build di debug
conservando i dati, perché la chiave è la stessa.

Aggiornamenti dall'app (2026-11-18):

- Con un tag, il job `android` allega l'APK firmato alla release in bozza
  come `Greatnote.arm64.apk` (attende che il job Windows crei la bozza).
- L'app (`update_commands_mobile.rs`, `greatnote_core::updates::package`)
  legge `latest.json` all'avvio dopo 15 secondi e ogni 6 ore, se gli
  aggiornamenti automatici sono attivi; se la versione è più recente scarica
  l'APK in `<cache>/update/` e chiede se installarlo (una volta per versione
  a ogni avvio). Impostazioni › Aggiornamenti è visibile anche su mobile.
- «Aggiorna» salva le bozze e apre l'installatore di Android
  (`FilesPlugin.installPackage`). La prima volta Android porta
  all'impostazione «Installa app sconosciute» per Greatnote.
- Le build di debug non si aggiornano.
- **Non provato su un telefono**: serve una release pubblicata con l'APK.
- Con Google Play questa funzione va tolta: Play non ammette
  `REQUEST_INSTALL_PACKAGES` per aggiornare l'app fuori dallo store.

Non fatto:

- Chiave di rilascio, account Play, scheda dello store e informativa sulla
  privacy pubblicata: decisioni dell'editore, non prese.
- Build per `armv7` (telefoni a 32 bit): non provata.
- Build nella CI: aggiunto il job `android` a
  `.github/workflows/installers.yml` (APK e AAB arm64 come artefatti, mai
  nella release). Firma con i segreti `GREATNOTE_ANDROID_KEYSTORE_BASE64`
  (il file `.jks` in base64), `GREATNOTE_ANDROID_KEYSTORE_PASSWORD`,
  `GREATNOTE_ANDROID_KEY_ALIAS` e `GREATNOTE_ANDROID_KEY_PASSWORD`; senza,
  i pacchetti escono non firmati. Usa l'NDK preinstallato sul runner.

## 7. Decisioni prese

- **Stesso repository** (in linea con l'ADR 0024): il progetto Android è
  generato in `src-tauri/gen/android`, il codice Rust e il frontend restano
  unici. Le differenze vanno in `tauri.android.conf.json` e in blocchi `cfg`.
- **Identificatore dell'app**: `dev.greatnote.notes`, lo stesso del desktop.
- **`src-tauri/gen/` è oggi ignorato da git.** Il progetto Android generato
  contiene però file che vanno versionati (manifest, configurazione Gradle,
  eventuale codice Kotlin). La regola in `.gitignore` andrà ristretta a
  `src-tauri/gen/schemas/` quando il progetto viene generato.

## 8. Ambiente di sviluppo

Ambiente con cui sono state eseguite le prove (Fedora 45):

| Componente | Versione |
|---|---|
| Rust (tramite `rustup`) | 1.99.1, con i quattro target Android |
| Tauri CLI | 2.12.2 |
| NDK | 30.0.16248370 |
| Android SDK Platform | 36 (richiesta dai plugin, installata da Gradle) e 37 |
| Build-Tools | 36.0.0 |
| JDK | OpenJDK 25.0.4 incluso in Android Studio |
| Emulatore | 37.2.13, immagine Android 17 x86_64 |

Valori del progetto generato da Tauri 2.12: `minSdk` 24 (Android 7.0),
`targetSdk` e `compileSdk` 37.

Requisiti secondo la documentazione di Tauri 2 (verificati sulla pagina dei
prerequisiti):

- Android Studio, e dal suo SDK Manager: Android SDK Platform, Platform-Tools,
  NDK (Side by side), Build-Tools, Command-line Tools.
- Variabili d'ambiente:

  ```sh
  export JAVA_HOME=/opt/android-studio/jbr
  export ANDROID_HOME="$HOME/Android/Sdk"
  export NDK_HOME="$ANDROID_HOME/ndk/$(ls -1 $ANDROID_HOME/ndk)"
  ```

  `JAVA_HOME` punta al JDK incluso in Android Studio: il percorso dipende da
  dove viene installato.

- Target Rust:

  ```sh
  rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
  ```

  Questo comando richiede `rustup`: con il Rust dei pacchetti Fedora i target
  Android non sono installabili.

Su Windows i requisiti sono gli stessi; cambiano solo i percorsi
(`%LocalAppData%\Android\Sdk`, `C:\Program Files\Android\Android Studio\jbr`).

### Comandi eseguiti e verificati

```sh
# APK di debug per l'emulatore (x86_64) o per un telefono (aarch64)
npx tauri android build --debug --target x86_64 --apk
npx tauri android build --debug --target aarch64 --apk

# Installazione e avvio
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
adb shell monkey -p dev.greatnote.notes -c android.intent.category.LAUNCHER 1

# Log dell'app: quelli di Rust hanno l'etichetta RustStdoutStderr,
# quelli della webview Tauri/Console
adb logcat -s RustStdoutStderr Tauri/Console

# Controllo dei tipi senza Gradle
T=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
CC_x86_64_linux_android=$T/x86_64-linux-android24-clang AR_x86_64_linux_android=$T/llvm-ar \
  cargo clippy -p greatnote-desktop --target x86_64-linux-android -- -D warnings
```

Solo il target `aarch64` non è ancora stato compilato.

Problema noto dell'emulatore su questa macchina: avviato senza finestra
(`-no-window`, con o senza `-gpu swiftshader_indirect`) termina con un
segmentation fault durante l'avvio. Funziona con la finestra e la GPU
dell'host:

```sh
emulator -avd Medium_Phone_API_37.0 -no-snapshot -gpu host -feature -Vulkan
```

Gradle usa lo script `npm run tauri`, aggiunto a `package.json`.

## 9. Questioni aperte

1. Versione minima di Android: resta il valore predefinito (Android 7.0)
   finché una prova reale non mostra un motivo per alzarla.
2. Priorità tra promemoria ad app chiusa e sincronizzazione Dropbox dopo la
   prima versione.
3. Nome dell'app nel progetto Android: il tema e la libreria si chiamano
   ancora `greatnote_desktop`, dal nome del crate.

## 10. Registro delle verifiche

| Data | Cosa | Esito |
|---|---|---|
| 2026-11-12 | Analisi del codice e delle dipendenze | Completata |
| 2026-11-12 | `cargo clippy` per `x86_64-linux-android` | Nessun avviso |
| 2026-11-12 | APK di debug x86_64 | Generato |
| 2026-11-12 | Avvio su emulatore (Android 17, x86_64) | L'app parte, crea `gna.sqlite3` nella cartella dati e mostra la schermata iniziale; nessun errore nella console. Layout desktop: contenuto tagliato a destra, barra dei menu sotto la barra di stato |
| 2026-11-12 | `cargo fmt`, `clippy`, `cargo test` su Linux | Verdi (236 + 13 test) |
| 2026-11-12 | Emulatore: nuova nota, scrittura con tastiera virtuale, ritorno all'elenco | Funziona; la tastiera non copre il fondo della nota |
| 2026-11-12 | Emulatore: chiusura forzata e riapertura | La nota e il testo sono presenti |
| 2026-11-12 | Emulatore: tasto Indietro (dialog, navigazione, nota, Home, uscita) | Funziona |
| 2026-11-12 | Emulatore: rotazione in orizzontale | La nota resta leggibile e modificabile |
| 2026-11-12 | `npm run test:ui` (interfaccia desktop) | Superato |
| — | Sospensione prolungata con app in background | Non provata |
| 2026-11-13 | Pixel 7 Pro: portachiavi (Android Keystore), voce di prova | Scrittura, lettura ed eliminazione riuscite |
| 2026-11-13 | Pixel 7 Pro: «Condividi con un'altra app…» da una nota | Si apre il pannello di Android con titolo e testo della nota |
| 2026-11-13 | Pixel 7 Pro: impostazioni Sincronizzazione e Integrazioni | Dropbox con «Collega Dropbox» attivo; calendari iCal |
| 2026-11-13 | Pixel 7 Pro: accesso a Dropbox e prima sincronizzazione | Collegato; 495 note ricevute da tre dispositivi. Segnalati 10 falsi conflitti (corretti nel codice, da riprovare su un dispositivo che entra da zero) |
| 2026-11-13 | Pixel 7 Pro, prova manuale: calendario iCal di Google e sincronizzazione Dropbox | Funzionano entrambi |
| 2026-11-13 | Pixel 7 Pro: impostazioni AI e vista Assistente | Visibili e coerenti nelle schermate; nessuna richiesta a un server |
| — | Pixel 7 Pro: connessione a un server AI, chat, ricerca per significato | Da provare |
| 2026-11-18 | Pixel 7 Pro, 0.2.10 di rilascio: promemoria alle 17:00 con app in background | Notifica arrivata alle 17:02 (finestra degli allarmi non esatti), icona e colore di Greatnote. Prima non arrivava: permesso delle notifiche mai chiesto |
| 2026-11-18 | vivo X300 Pro (Android 16), 0.2.10: promemoria con app chiusa dalle recenti | Non arriva: allarme rimosso con motivo `frozen`. Dopo l'esenzione dall'ottimizzazione della batteria arriva puntuale |
| 2026-11-13 | Pixel 7 Pro: promemoria su una nota alle 11:10, app in background e processo terminato | Notifica «Nota da android — Promemoria delle 11:10» arrivata alle 11:11:47 con l'app chiusa; riaprendo, il promemoria è «da gestire» |
| 2026-11-13 | Pixel 7 Pro: permesso delle notifiche dalla vista Promemoria | Avviso mostrato senza permesso, sparito dopo averlo concesso |
| 2026-11-13 | Pixel 7 Pro: viste Attività, Calendario (mese e settimana), dialog di attività ed evento | Leggibili e con controlli da 44px nelle schermate; uso a mano non ancora provato |
| 2026-11-13 | Pixel 7 Pro (Android 17, arm64): build `aarch64`, installazione, avvio | Funziona; italiano e tema scuro del telefono |
| 2026-11-13 | Pixel 7 Pro, prova manuale: scrittura, foto e allegati, tasto Indietro | Funzionano |
| 2026-11-13 | Pixel 7 Pro, prova manuale: barra della formattazione, menu «Inserisci», pressione prolungata sul testo, pulsanti dei taccuini | Segnalati come poco usabili o difettosi; corretti, **da riprovare** |
| 2026-11-13 | Pixel 7 Pro: barra in fondo e sopra la tastiera, menu come pannello dal basso | Visti nelle schermate dopo la correzione |
| 2026-11-13 | Pixel 7 Pro: campo dei tag con tastiera aperta, proposta «Crea il tag» | Il campo resta attivo e la proposta compare |
| 2026-11-13 | Pixel 7 Pro: campo data dei promemoria nella nota | Intero, su una riga; ora e minuti sotto |
| 2026-11-12 | Emulatore: ripristino da file di un backup creato su Linux (nota con accenti e caratteri giapponesi, un allegato) | Verificato «integro: 1 nota, 1 allegato», ripristinato; nota e allegato presenti |
| 2026-11-12 | Emulatore: backup cifrato (`.zip.age`) creato su Linux | Riconosciuto come cifrato, sbloccato con la password, verificato integro |
| 2026-11-12 | Emulatore: «Salva una copia» di un backup in Download | File scritto; sul computer `unzip -t` senza errori e ripristino con il core desktop riuscito, con la nota scritta sull'emulatore |
| 2026-11-12 | Emulatore: importazione di un file Markdown | Anteprima «1 nuova», importata con il grassetto |
| 2026-11-12 | Emulatore: esportazione di una nota in Markdown | File corretto, con intestazione e formattazione |
| 2026-11-12 | Emulatore: aggiunta di un allegato dal selettore di sistema | Allegato salvato |
| 2026-11-12 | Emulatore: apertura di un allegato | Compare «Apri con» e l'altra app riceve il file |
| — | Portachiavi, notifiche, selettore di file su Android | Non provati |
| — | Build `aarch64` e avvio su vivo X300 Pro | Non eseguiti |
| 2026-11-16 | Build di rilascio `aarch64` (APK + AAB) e `x86_64` (APK) | Generate; APK arm64 24 MB, AAB 21 MB |
| 2026-11-16 | Pixel 7 Pro: APK di rilascio (R8) firmato con la chiave di debug, installato sopra la build di debug | La prima prova non partiva: `plugins.notification` in `tauri.android.conf.json` è rifiutato dal plugin 2.6.0. Tolto; l'app parte con i dati esistenti, nessun errore nel log |
| 2026-11-16 | Pixel 7 Pro: riapertura della nota dopo chiusura forzata | Prima versione non funzionante (restava sulla Home), corretta; la verifica della correzione è stata interrotta dal blocco dello schermo: **da riprovare** |
| 2026-11-16 | Emulatore | Si chiude durante l'installazione (errori gfxstream), prova spostata sul telefono |
| — | Build Windows dopo le modifiche | Non eseguita (solo CI) |
