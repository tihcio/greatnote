# Test e controlli

## Core Rust

Il core è separato dalla shell Tauri: i test delle migrazioni e dei repository
SQLite si possono eseguire senza un desktop o librerie GTK/WebKit.

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Il workspace Rust include anche la shell Tauri, ma per impostazione predefinita
il comando `cargo test` esegue il core senza compilare componenti grafici.

Prima di un rilascio `scripts/release-check.sh` esegue tutti i controlli
automatici (workspace completo, `cargo audit`, build e audit npm); vedi la
[checklist di rilascio](./RELEASE_CHECKLIST.md).

## Frontend

```sh
npm ci
npm run build
```

La compilazione TypeScript con `strict` verifica componenti, bridge e
configurazione Vite. La UI non deve sostituire con dati dimostrativi i
comandi Tauri: al di fuori della shell desktop comunica chiaramente che il
backend locale non è disponibile.

Per verificare l'editor, avviare `npm run tauri:dev` e controllare che la
finestra carichi la UI e che il terminale Vite non segnali errori React/Tiptap.
La sincronizzazione dell'editor deve ignorare istanze Tiptap distrutte.

## Shell desktop Linux

```sh
npm run tauri:dev
npm run tauri:build
```

Su Fedora, Tauri richiede i pacchetti di sviluppo GTK 3, WebKitGTK 4.1 e
librerie associate. Verificare le dipendenze correnti nella documentazione
Tauri per la propria versione di Fedora.

## Principi per le fixture

- Usare solo note sintetiche nei test.
- Usare database temporanei/in-memory e verificare errori, vincoli e
  persistenza tra connessioni.
- Non usare account, servizi remoti, credenziali o dati personali reali.

## Organizzazione e cestino

I test storage verificano CRUD di spazi/taccuini/tag, assegnazione e filtro
per tag, rollback delle associazioni tag non valide, scollegamento delle note
senza perdita dei contenuti e ripristino di note cestinate dopo la riapertura
del database. Verificano inoltre rifiuto di descrizioni eccessive e
migrazione v1→v4 senza perdita delle note esistenti.

Verifica manuale in `npm run tauri:dev`: crea spazio e taccuino, crea nota e
assegnala al taccuino, applica un tag, filtra dalla barra laterale, sposta la
nota nel cestino, riapri la nota dal cestino e ripristinala. Chiudi e riavvia
l'app per controllare che note e organizzazione persistano.

Il test `drafts_survive_reopening_and_are_cleared_when_applied` simula una
bozza durevole non ancora applicata, riapre il database e verifica che la
versione salvata non sia sovrascritta, poi controlla che applicazione e
scarto eliminino la bozza. L'interfaccia chiede conferma quando apre una nota
che possiede una bozza recuperabile.

## Ricerca e produttività

`cargo test -p greatnote-core` verifica migrazioni v1→v5 (con backup
pre-migrazione e reindicizzazione delle note cestinate escluse dalla
ricostruzione v3→v4), incluso l'aggiornamento di
database v3 che non hanno le colonne dell'indice allegati o
`attachments.search_text`, disponibilità FTS5,
indicizzazione iniziale, aggiornamento/rimozione dei termini al salvataggio,
frasi e caratteri speciali, parser dei filtri, viste preferite/archivio/
recenti e ricerche parametrizzate. La sintassi booleana non supportata deve
restituire un errore e non una ricerca parziale.

I test verificano inoltre upload testuale, indicizzazione dei primi 100.000
caratteri, magic bytes, nome file normalizzato, deduplicazione, verifica hash,
cancellazione e rimozione dei termini FTS. I test dei collegamenti verificano
auto-link rifiutato, destinazione inesistente, aggiornamento idempotente
dell'etichetta e rimozione.

Verifica manuale in `npm run tauri:dev`: cerca una frase e prova i filtri
`tag:`, `notebook:`, `space:`, `created:`, `updated:`, `has:attachment` e
`is:pinned`; controlla l'errore per virgolette non chiuse e `OR`; marca una
nota preferita, apri Recenti, archivia e ripristina; allega TXT/PDF e un file
con estensione ingannevole; apri/rimuovi l'allegato; collega una nota, naviga
alla destinazione e rimuovi il link. Prova le scorciatoie nell'editor e nella
ricerca.

## Attività, promemoria e calendario

`cargo test -p greatnote-core` verifica:

- `time`: conversione orario locale → UTC in inverno ed estate, orario
  inesistente (29/03/2026 02:30 Europe/Rome → 03:30) e ripetuto
  (25/10/2026 02:30 → prima occorrenza), giorno di 25 ore, "domani alla
  stessa ora" attraverso il ritorno all'ora solare, "oggi" diverso fra Roma e
  New York, input malformati e fusi inesistenti o con percorsi;
- `planner`: viste delle attività rispetto a una data data, validazione,
  collegamento a note (anche nel cestino), ricerca con caratteri speciali
  trattati come testo, completamento che chiude i promemoria; promemoria che
  scattano una sola volta anche se persi, che non scattano per note nel
  cestino o attività completate e riprendono dopo il ripristino; rinvio a
  domani attraverso il cambio d'ora; eventi per giorno locale in fusi diversi,
  validazione e trigger di eliminazione dei promemoria; persistenza dopo la
  riapertura del database; filtro `is:task`;
- `transfer`: round-trip dell'archivio con attività, eventi e promemoria,
  promemoria già passati importati senza notifiche arretrate e nessun
  duplicato alla seconda importazione.

`cargo test -p greatnote-desktop` verifica il testo delle notifiche (puntuale
o perso).

Verifica manuale in `npm run tauri:dev`:

1. Crea un'attività per oggi, una in ritardo (data passata) e una senza data:
   controlla schede e contatore nella barra laterale; completane una.
2. In una nota aggiungi un'attività collegata e un promemoria fra 1–2 minuti:
   deve comparire la notifica di sistema e il riquadro in basso a destra;
   prova +10 min, Domani e Fatto.
3. Imposta un promemoria fra 1–2 minuti, chiudi l'app, attendi almeno 2
   minuti dopo l'orario e riaprila: entro pochi secondi arriva la notifica
   "Scaduto alle …: perso mentre l'app era chiusa" e il riquadro lo indica.
   Riaprendo di nuovo senza gestirlo arriva il riepilogo "Hai 1 promemoria da
   gestire". Con `RUST_LOG=info npm run tauri:dev` il log riporta "notifica di
   sistema inviata" oppure l'errore di consegna.
4. In un'attività e in un evento aggiungi un promemoria scegliendo ore e
   minuti: il dialog resta aperto e il promemoria compare nell'elenco.
5. Nel calendario crea un evento con orario, uno di più giorni interi e
   verifica le viste mensile e settimanale e la scadenza delle attività.
6. Avvia con un altro fuso (`TZ=America/New_York npm run tauri:dev`) e
   controlla che eventi e promemoria siano mostrati all'ora locale
   corrispondente mentre le scadenze restano sulla stessa data.

## Cestino

`cargo test -p greatnote-core` (`trash`, `storage`, `transfer`) verifica:
taccuino nel cestino con note rese "senza taccuino" e ricollegate al
ripristino (salvo quelle spostate nel frattempo), divieto di usare contenitori
cestinati, spazi e tag con collegamenti ricreati, nomi dei tag unici anche
rispetto al cestino, attività ed eventi esclusi da viste, calendario e
promemoria finché sono nel cestino, eliminazione definitiva solo dal cestino,
svuotamento completo che lascia intatte le note dei taccuini eliminati,
eliminazione definitiva delle note con file allegati, e tag cestinati
ripristinati da un'importazione.

Verifica manuale: sposta nel cestino un taccuino con note, uno spazio, un tag
assegnato, un'attività con promemoria e un evento; controlla la sezione
"Altri elementi", ripristinali e verifica che note, taccuini e tag tornino al
loro posto; poi usa **Svuota cestino**.

## Importazione da altre app

Test in `src/external_import/` con campioni sintetici:

- ENEX: note, tag, date, risorse collegate per MD5, caselle, testo cifrato,
  script rimossi, titolo non duplicato, file non validi, entità esterne non
  risolte;
- JEX: proprietà in coda, cartelle annidate, tag, risorse in Markdown e
  HTML, collegamenti, link nei blocchi di codice lasciati intatti, file non
  JEX;
- Obsidian: cartelle nascoste ignorate, front matter (varianti), `#tag`
  fuori dal codice, wikilink con alias e percorsi, link relativi con `..`,
  incorporamenti, tipi non supportati, file fuori dal vault e collegamenti
  simbolici rifiutati;
- integrazione: anteprima, commit con spazio e taccuini annidati, tag,
  allegati, collegamenti, date e duplicati alla seconda importazione;
  destinazione piatta.

Il 2026-10-19 l'anteprima è stata eseguita su un backup JEX e un vault
Obsidian reali dell'utente (solo conteggi, nessuna importazione): 488 e 486
note senza errori.

## Immagini e testo nelle immagini

- documento: nodo `image` solo con id di allegato (URL, percorsi e
  attributi estranei rifiutati), Markdown con il percorso dell'allegato;
- importazione: segnaposto delle immagini convertiti in immagini (anche
  nelle voci di elenco), immagini riferite agli allegati importati,
  duplicati riconosciuti dopo la conversione;
- OCR: testo salvato, ricercabile, usato dall'assistente, errori e
  rilettura, solo immagini; provider: data URL per i server compatibili
  OpenAI, input `stream-json` con immagine per Claude Code;
- interfaccia: inserimento nel testo con immagine caricata e lettura del
  testo dal pannello allegati.

Prova reale facoltativa: `GREATNOTE_OCR_IMAGE=immagine.png cargo test --test
ai_live claude_code_reads -- --ignored --nocapture`.

## Testo cifrato

- `secret_text`: andata e ritorno solo con la password giusta, sale diverso
  a ogni cifratura, password corta, testo vuoto o oltre 32 KiB, testo
  alterato o non age, suggerimenti su una riga;
- documento: `encryptedText` solo con testo age valido e suggerimento (testo
  in chiaro, attributi estranei e contenuto figlio rifiutati); testo
  semplice con il solo «🔒»; Markdown e HTML con il blocco `age` che torna
  testo cifrato all'importazione; un blocco `age` non cifrato resta codice;
- revisioni: eliminazione della cronologia di una sola nota;
- interfaccia: cifratura della selezione con `Ctrl+Shift+X`, nota senza il
  testo in chiaro, cronologia eliminata dopo il salvataggio, password errata
  e giusta, chiusura con Esc.

## Menu contestuali

Test dell'interfaccia: tasto destro sul Cestino (con **Svuota cestino**), su
un taccuino, sull'intestazione dei tag, su una scheda, nel testo della nota
(la voce **Seleziona tutto** esegue l'azione e chiude il menu) e nel campo
di ricerca (menu standard dei campi di testo).

## Funzioni AI

I test del modulo `src/ai/` usano server HTTP finti (`provider::fake_server`)
e un provider di embedding deterministico, mai servizi reali:

- impostazioni: AI spenta per impostazione predefinita, indirizzi verificati
  (solo http/https, niente credenziali o query, https per i server remoti),
  consenso obbligatorio per i server remoti;
- provider: elenco modelli, chiave solo nell'intestazione `Authorization`,
  streaming SSE con rimozione di `<think>`, risposte non in streaming, 401,
  404 con messaggio, 429 con `Retry-After`, embedding ordinati e validati,
  annullamento e server irraggiungibile;
- indice: indicizzazione, ricerca, note modificate escluse finché non
  reindicizzate, metadati che non ricalcolano gli embedding, esclusioni,
  cestino, cambio modello, eliminazione definitiva, errori del server che
  fermano il ciclo e note rifiutate che no;
- recupero: termini FTS sicuri, fonti per parole chiave e semantiche,
  rilettura del testo corrente, budget di caratteri;
- planner come contesto (`ai::planner_context`): espressioni di tempo in
  italiano e inglese trasformate in intervalli di giorni (oggi, domani,
  settimane, fine settimana, mesi con anno più vicino, giorni della
  settimana, date numeriche, «prossimi N giorni» con limite), parole del
  planner e termini di ricerca; eventi e attività del periodo con orari nel
  fuso dell'utente e promemoria inclusi nella fonte; attività in ritardo per
  «oggi»; ricerca per parole a qualunque data; taccuini esclusi, cestino,
  impostazione disattivata e budget; rilettura che riapplica esclusioni e
  cestino;
- prompt: delimitatori non falsificabili, cronologia limitata, analisi
  difensiva di tag e attività, tipo della fonte e data corrente.

Aggiornamenti in-app: `updates` verifica la preferenza (predefinita
attiva, persistente, valore danneggiato ignorato). Il test
dell'interfaccia (passo `aggiornamenti`) cerca un aggiornamento simulato,
mostra versione e novità, il pulsante nella barra laterale e la richiesta di
installazione dopo il salvataggio. `scripts/updater-manifest.ps1` è stato
provato su un installer finto firmato senza versione (rifirma e
`latest.json`). Il download e l'installazione reali richiedono una release
pubblicata: vedi la checklist di rilascio.

Il test dell'interfaccia (`npm run test:ui`) verifica che una fonte
«attività · per data» compaia sotto la risposta e che la sua citazione apra
l'attività nella vista Attività.

Prova reale facoltativa con un server locale (ignorata di default):

```sh
GREATNOTE_AI_URL=http://localhost:11434/v1 GREATNOTE_AI_CHAT_MODEL=qwen3:8b \
  cargo test --test ai_live -- --ignored --nocapture
# con un modello di embedding: GREATNOTE_AI_EMBED_MODEL=nomic-embed-text
```

Eseguita il 2026-10-20 con `qwen3:8b` e `qwen3-embedding:8b` su Ollama:
indicizzazione di 2 note, fonte corretta per similarità (0,64 contro 0,23),
risposta con citazione `[1]`, istruzioni ostili ignorate e confronto tra due
versioni con le modifiche corrette (budget e referente).

Portachiavi di sistema (Secret Service: GNOME Keyring o KWallet), con un
valore fittizio poi rimosso:

```sh
cargo test -p greatnote-desktop keyring -- --ignored
```

Eseguita il 2026-10-20 su KDE Plasma (KWallet): scrittura, lettura e
cancellazione riuscite.

Prova reale del motore integrato con il `llama-server` incluso (ADR 0020,
ADR 0024). Scarica una volta `qwen3.5-0.8b` e `bge-m3` (circa 1,5 GB) nella
cartella indicata:

```sh
bash scripts/fetch-llama.sh
GREATNOTE_LLAMA_SERVER=$PWD/src-tauri/resources/llama/llama-server \
GREATNOTE_AI_MODELS_DIR=$PWD/target/ai-models \
  cargo test --test ai_live builtin_engine -- --ignored --nocapture
```

Eseguita il 2026-10-22 su Fedora 44 (RTX 3070, Vulkan), con questi
risultati:

- risposta corretta («Roma») e embedding da 1024 dimensioni;
- con il registro dettagliato, llama.cpp ha usato solo la GPU dedicata e
  ha caricato in VRAM tutti i 26 strati;
- velocità: 210 token/s in generazione, 762 token/s nella lettura del
  prompt.

Prova reale facoltativa con Claude Code (accesso già effettuato):

```sh
cargo test --test ai_live claude_code -- --ignored --nocapture
```

Eseguita il 2026-10-19 con `haiku`: risposta corretta con citazioni `[1]`
in 2,7 s, istruzione ostile nella nota ignorata. I test unitari usano un
finto eseguibile `claude` (argomenti senza strumenti, testo solo su stdin,
cartella temporanea rimossa, errori di accesso, annullamento).

Il 2026-10-19 è stata eseguita con `qwen3:8b` su un server locale: risposta
con citazione `[1]` della nota corretta e testo con istruzioni ostili
trattato come dato. Il server usato non supportava gli embedding: l'errore
501 viene mostrato e ferma l'indicizzazione.

Verifica manuale nella finestra nativa: salvataggio e cancellazione della
chiave nel portachiavi (KWallet/GNOME Keyring), indicizzazione con un
modello di embedding reale, annullamento di una risposta lunga.

## Sincronizzazione

Test in `src/sync/` con più dispositivi simulati (database su file distinti e
cartella condivisa temporanea):

- **Scambio completo e idempotenza:**
  - spazi, taccuini, note, tag, attività, eventi, promemoria;
  - ricerca sul secondo dispositivo;
  - giro senza modifiche che non fa nulla;
  - modifiche applicate che non rimbalzano.
- **Conflitti e ordine:**
  - modifiche contemporanee: stessa nota su entrambi e testo perdente nella
    cronologia di entrambi;
  - una modifica prevale su un'eliminazione definitiva contemporanea;
    un'eliminazione senza conflitti arriva a tutti, indice compreso;
  - un lotto con dipendenze non ancora arrivate attende e viene applicato
    dopo.
- **Allegati e tag:**
  - tag con lo stesso nome creati su due dispositivi e unificati;
  - allegato con blob in ritardo: in attesa, poi copiato e identico byte
    per byte; rimozione propagata con il file.
- **Sicurezza:**
  - database copiato rilevato, poi riunito con una nuova identità;
  - abbonamenti iCal esclusi;
  - salvataggio dell'editor su una versione superata: testo remoto
    conservato;
  - operazione ostile (nome di file con percorso) scartata senza lasciare
    righe.
- **Struttura:**
  - `tables`: ogni colonna esiste nello schema, ogni tabella ha i suoi tre
    trigger, istantanee e riferimenti mancanti;
  - `folder`: lotti immutabili, file troncati ignorati, blob verificati,
    formato più recente rifiutato.

Dropbox (`src/sync/dropbox.rs`), con un server finto:

- URL di autorizzazione con PKCE e `token_access_type=offline`;
- scambio del codice senza segreto dell'app e `invalid_grant` che chiede
  di ricollegare;
- account;
- download e upload binari, conflitto in creazione, esistenza, elenco su
  due pagine, cartella assente;
- nuovo tentativo dopo un 429, 401 segnalato, percorsi con `..` rifiutati.

Il protocollo completo è provato anche su un archivio remoto in memoria
(`remote_store_carries_notes_and_attachments`).

Prova reale eseguita il 2026-10-20 con l'account Dropbox dell'utente:

- login nel browser riuscito, dopo aver registrato esattamente
  `http://localhost:41927/` come Redirect URI;
- primo invio di 787 elementi (490 note e 164 allegati) completato senza
  errori.

La prova ha fatto emergere un difetto: il caricamento in rete avveniva con
il database bloccato, rendendo l'app poco reattiva. Ora è eseguito fuori dal
blocco ed è coperto da `network_calls_never_hold_the_database_lock`.

Prova su dati reali (copia, mai l'originale):

```sh
GREATNOTE_SYNC_SOURCE_DIR=/percorso/copia-dati \
  cargo test --release --test sync_live -- --ignored --nocapture
```

Eseguita il 2026-10-20 su una copia dei dati dell'utente:

| Misura | Risultato |
|---|---|
| Esportazione | 782 operazioni in 44 ms (cartella 20 MB) |
| Importazione su un dispositivo nuovo | 0,2 s, 0 operazioni scartate, 0 conflitti |
| Note | 487 attive, 1 archiviata, 2 nel cestino, identiche |
| Allegati | 164, identici byte per byte |
| Giro senza modifiche | 0,2 ms |

## Calendari iCal

Test in `src/calendar_sync/ics.rs` e `subscriptions.rs`:

- **Lettura:**
  - righe ripiegate, escape, `X-WR-CALNAME`;
  - fusi IANA, alias `VTIMEZONE` e nomi Windows di Outlook;
  - giorni interi, `DURATION`, eventi annullati o fuori finestra.
- **Ricorrenze:**
  - settimanali con `EXDATE` e `RECURRENCE-ID` attraverso il cambio
    dell'ora legale;
  - ultimo venerdì del mese con `UNTIL`;
  - 29 febbraio annuale;
  - ogni due giorni;
  - ultimo giorno lavorativo (`BYSETPOS`);
  - regola non supportata ridotta alla prima occorrenza;
  - regola infinita dal 1900 limitata e saltata alla finestra.
- **Abbonamenti:**
  - indirizzi accettati e rifiutati;
  - sola lettura e blocco del cestino;
  - promemoria conservati;
  - eventi spariti rimossi;
  - errore registrato senza perdere gli eventi;
  - rinomina e colore;
  - rimozione completa;
  - download HTTP con 404 e reindirizzamento rifiutati.
- **Interfaccia:**
  - evento iCal colorato e in sola lettura;
  - indirizzo non valido rifiutato;
  - aggiunta di un abbonamento.

Prova reale (rete):

```sh
cargo test --test calendar_live -- --ignored --nocapture
```

Eseguita il 2026-10-20 con il calendario pubblico «Festività in Italia» di
Google: 23 eventi nella finestra (Tutti i Santi, Immacolata, Natale…) con
date corrette. Migrazione 13 applicata dall'app al database reale: 490 note,
integrità e chiavi esterne verificate, backup pre-migrazione creato.

## Calendari esterni

Test in `src/calendar_sync/`, senza account reali:

- **OAuth:**
  - PKCE (S256) e URL di autorizzazione;
  - lettura del ritorno dal browser, accesso negato, richieste estranee;
  - scambio e aggiornamento del token contro un server finto (campi
    inviati, `invalid_grant` → ricollegare, errori leggibili).
- **Google e Microsoft:**
  - conversione degli eventi: fusi con offset, giorni interi con fine
    esclusiva, ricorrenze, annullati;
  - corpi di creazione;
  - richieste HTTP reali verso un server finto: paginazione, `If-Match`,
    412 → conflitto, 404/410 su eliminazione, 401;
  - `nextLink` verso un altro host rifiutato.
- **Sincronizzazione** con un fornitore in memoria:
  - calendari selezionati;
  - nota collegata e promemoria conservati;
  - conflitto ETag;
  - eventi e calendari spariti rimossi con i promemoria;
  - calendari in sola lettura;
  - creazione senza duplicati alla sincronizzazione successiva;
  - scollegamento;
  - blocco delle modifiche locali e del cestino.
- **Desktop:** `calendar_commands` verifica:
  - il ritorno sul loopback con `state` errato ignorato e l'annullamento;
  - la validazione delle registrazioni incorporate.

  Verificato il 2026-10-20 che un ID passato con
  `GREATNOTE_MICROSOFT_CLIENT_ID` finisce nel binario e sparisce
  ricompilando senza la variabile.
- **Interfaccia:** test UI su evento esterno colorato, occorrenza in sola
  lettura, modifica inviata al fornitore, scelta del calendario, impostazioni
  con account e calendari.
- **Migrazione 12:** verificata il 2026-10-20 su una copia del database
  reale (integrità e chiavi esterne).

Prova reale: richiede app registrate presso Google/Microsoft (vedi
USER_GUIDE). Non è ancora stata eseguita.

## Server MCP

Test unitari in `src/mcp.rs`, su un database temporaneo riaperto in sola
lettura:

- handshake e negoziazione della versione, notifiche senza risposta,
  metodi sconosciuti, JSON non valido e batch rifiutati;
- ricerca e lettura che escludono i taccuini esclusi dall'AI (anche per id
  diretto), id non validi, limiti di `limit` e lunghezza della query;
- server disattivato per impostazione predefinita, connessione che non può
  scrivere, schema diverso non migrato;
- registro senza testo e rotazione oltre 1 MB; ciclo stdin/stdout fino a
  EOF.

Prova manuale dell'eseguibile (usa una copia del database, mai
l'originale):

```sh
cargo build -p greatnote-desktop
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_tags","arguments":{}}}' \
  | target/debug/greatnote-desktop --mcp --data-dir /percorso/copia
```

Eseguita il 2026-10-20 su una copia del database reale: risposta
«disattivato» prima dell'attivazione, poi tag, ricerca e taccuini
restituiti; il registro conteneva solo strumenti e conteggi.

## Interfaccia (test automatico)

```sh
npm run test:ui
```

`npm run ui:screens` salva in `target/ui-screens/` le viste principali in tema
chiaro e scuro (stesso banco di prova, nessuna verifica) per la revisione
grafica.

Avvia Vite sulla porta 1435 e apre `frontend/test-harness.html` (backend
Tauri simulato con dati sintetici, escluso dalla build) in Firefox headless
tramite `puppeteer-core` e WebDriver BiDi, senza scaricare browser
(`FIREFOX=/percorso/firefox` per un eseguibile diverso). Verifica Home, menu
File/Visualizza/Nota con cambio tema, apertura di una nota con barra degli
strumenti, tag dalla barra in basso (suggerimento, creazione, rimozione), menu Inserisci, comandi "/", trova e sostituisci con
evidenziazione, menu contestuale, informazioni nota, modifica tag, sposta
in, Vai a…, impostazioni, nuova attività, schede e indietro, le viste
Modelli/File/Calendario/Promemoria/Cestino, vista concentrazione, elenco
delle scorciatoie, assistente AI (chat con citazione e fonti, ricerca per
significato), modifica AI con anteprima, confronto e applicazione nell'editor,
tag suggeriti, impostazioni AI con prova di connessione (backend AI
simulato) e finestra separata. Fallisce su un passaggio non riuscito
o su errori JavaScript; gli screenshot sono in `target/ui-smoke/`.

Il test copre il frontend; l'integrazione con Rust è coperta dai test del
core (`library`: revisioni, modelli, copie con allegati indipendenti, file,
contatori, preferenze; `content`: checklist, tabelle e stili esportati,
stili non sicuri rifiutati).

Verifica manuale nella finestra nativa: Modifica › Incolla e Incolla senza
formattazione, zoom (`Ctrl++`), `F11`, Nota › Apri in una nuova finestra
(modifica e chiusura), stampa, tema scuro forzato con sistema chiaro.

## Import/export e protezione dati

I test del core (fixture sintetiche e directory temporanee) coprono:

- `content`: round-trip Markdown con struttura, contrassegni, caratteri
  speciali e front matter; HTML ostile (script, gestori di eventi, iframe,
  `javascript:`), escape dell'HTML esportato, annidamento profondo, testo
  semplice e timestamp;
- `transfer`: nomi file portabili e riservati Windows, export di nota
  singola, round-trip ZIP Markdown → import cartella (date, tag, allegati,
  duplicati), round-trip archivio completo (spazi, taccuini, tag con nome già
  esistente, preferite, archivio, collegamenti, allegati, indice),
  rollback con rimozione degli allegati già scritti quando l'archivio è
  alterato tra anteprima e commit, path traversal/percorsi assoluti nel front
  matter, symlink, file nascosti, file non UTF-8 e HTML con script;
- `backup`: nomi dei backup, verifica e ripristino con backup di sicurezza e
  riapertura del database, backup alterati rifiutati senza toccare i dati,
  pianificazione/conservazione dei backup automatici, controllo d'integrità
  con allegati mancanti/danneggiati e file orfani; nomi legacy `gna-…`;
  copia cifrata age: password troppo corta, contenuto non leggibile in
  chiaro, doppia cifratura rifiutata, verifica/ripristino senza password
  rifiutati, password errata distinta da file alterato, nessuna copia
  decifrata residua dopo errore o uso, ripristino completo da copia cifrata e
  pulizia dei residui all'avvio. Nei test il work factor scrypt è ridotto
  (`cfg(test)`) per tenere bassi tempo e memoria;
- `storage`: backup creato da un database con schema 2 verificato,
  ripristinato su un'installazione nuova, migrato allo schema corrente,
  ricercabile e integro anche dopo la riapertura;
- `sync`: ripristino di un backup più vecchio dell'ultima sincronizzazione
  → sincronizzazione ferma senza scrivere nella cartella; con una nuova
  identità il dispositivo si ricongiunge e nessuna nota va persa;
- `data_dir`: spostamento dei dati da `dev.gna.notes` quando la cartella nuova
  contiene solo cache web o un database vuoto (messo da parte), nessuna
  modifica se entrambe contengono note, nessuna azione alla seconda
  esecuzione;
- shell (`cargo test -p greatnote-desktop`): ripiego sulla vecchia cartella
  quando lo spostamento non è possibile.

Verifica manuale in `npm run tauri:dev`:

1. Esporta una nota in Markdown, HTML, testo e PDF (dialog di stampa) e apri
   i file prodotti.
2. In *Dati e backup*, esporta un taccuino come ZIP Markdown, estrailo e
   reimportalo come cartella: controlla anteprima, duplicati, taccuino di
   destinazione, tag e allegati.
3. Esporta l'archivio completo e reimportalo in un profilo pulito
   (`XDG_DATA_HOME=$(mktemp -d) npm run tauri:dev`).
4. Crea un backup, modifica una nota, ripristina il backup e verifica che la
   finestra si ricarichi con i dati precedenti; ripristina poi il backup
   "Prima del ripristino".
5. Avvia la verifica d'integrità e la ricostruzione dell'indice.
6. Salva una **Copia cifrata…** di un backup (prova password corta e non
   coincidente), poi **Ripristina da file…** scegliendo il `.zip.age`:
   password errata → errore, password giusta → verifica e ripristino. Dopo
   annullamento o ripristino nella cartella dati non devono restare file
   `.decrypted-*`. Se disponibile, decifra la copia con `age -d` per
   confermare l'interoperabilità.
7. Con dati in `~/.local/share/dev.gna.notes/`, avvia l'app (anche se
   `dev.greatnote.notes/` esiste già con un database vuoto) e verifica che le
   note siano presenti e che `gna.sqlite3`, `attachments/` e `backups/` siano
   stati spostati.
