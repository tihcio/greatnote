# Roadmap

La priorità è mantenere ogni incremento utilizzabile e verificabile offline.
Le fasi non ancora implementate restano esplicitamente aperte.

## Fase 0 — Ricognizione e bootstrap

- [x] Leggere la specifica esistente e controllare ambiente e toolchain.
- [x] Documentare architettura, decisioni iniziali, sicurezza e test.
- [x] Creare lo scheletro Tauri 2, React e TypeScript.
- [x] Rendere la finestra adattabile e indipendente dal desktop environment.
- [x] Aggiungere CI per formattazione, Clippy, test Rust e build frontend.
- [x] Compilare frontend e testare il core; registrare il limite delle
      dipendenze native Linux e degli strumenti Rust mancanti.
- [x] Compilare la shell Tauri dopo l'installazione delle dipendenze native.
- [ ] Verificare manualmente finestra e flusso desktop end-to-end; l'utente
      ha avviato `tauri:dev` e Vite risponde su localhost.
- [x] Separare la CSP di sviluppo da quella del bundle per consentire React
      Refresh e HMR soltanto sul server Vite locale.
- [x] Eseguire localmente rustfmt e Clippy (`-D warnings`) sull'intero workspace.

**Accettazione:** struttura documentata, frontend compilabile e nucleo Rust
testabile; resta da provare manualmente il flusso completo nella finestra
nativa.

## Fase 1 — Core locale

- [x] Completare i repository SQLite e i comandi Tauri per spazi,
      taccuini, tag, assegnazioni e cestino senza sostituire il database
      esistente.
- [x] Collegare la gestione dell'organizzazione alla UI, includendo
      selezione/filtro e spostamento delle note.
- [x] Aggiungere test delle regole di cancellazione, integrità delle
      relazioni, ripristino e persistenza al riavvio.
- [x] Aggiornare guida utente, architettura e changelog con il comportamento
      effettivamente implementato.
- [x] Definire migrazione SQLite per note, spazi, taccuini e tag.
- [x] Aggiungere migrazione v2 per bozze e verificare l'upgrade da v1
      preservando note già esistenti.
- [x] Implementare repository e test d'integrazione per le operazioni base
      sulle note.
- [x] Collegare la UI a comandi Tauri per creare, leggere e modificare note.
- [x] Implementare l'editor strutturato, autosave e visualizzazione degli
      errori.
- [x] Aggiungere organizzazione completa, bozze, cestino e ripristino.
- [x] Eliminazione definitiva dal cestino e svuotamento del cestino, con
      conferma e rimozione dei file allegati.
- [x] Cestino anche per spazi, taccuini, tag, attività ed eventi, con
      ripristino dei collegamenti (migrazione v7, ADR 0008).
- [ ] Avviare e verificare il flusso desktop completo.
- [x] Documentare schema, formato contenuto e recupero dei dati.

**Accettazione:** creare, modificare e ritrovare note persistenti dopo il
riavvio, senza rete.

## Fase 2 — Ricerca e produttività

- [x] Aggiungere FTS5, filtri e parser di query validato.
- [x] Aggiungere preferite, archivio, recenti e scorciatoie (il cestino è
      incluso nel core locale).
- [x] Implementare allegati locali con percorsi e dimensioni convalidati.
- [x] Aggiungere link interni tra note.
- [x] Aggiornare README, guida, architettura, sicurezza e documentazione test.

**Accettazione:** core e parser coperti da test; filtri eseguiti con SQL
parametrizzato. La sintassi AND/OR/NOT e parentesi è esplicitamente rifiutata
con messaggio leggibile; non viene interpretata silenziosamente.

**Revisione 2026-10-14:** test e build verdi; query parametrizzate ed
evidenziazione sicura confermate. Corretto un difetto: la ricostruzione FTS
v3→v4 escludeva le note nel cestino, che una volta ripristinate non erano più
trovabili. La migrazione v5 le reindicizza e il ripristino dal cestino ora
aggiorna sempre l'indice (test di regressione aggiunto). Restano aperti, non
bloccanti: rinomina allegati, limite allegati configurabile, verifica
manuale nella finestra desktop.

## Fase 3 — Import/export e protezione dati

- [x] Conversione documento ⇄ Markdown/HTML/testo con whitelist di nodi e
      link sicuri (`src/content.rs`).
- [x] Importare Markdown (file e cartelle, front matter con tag/date/
      allegati), testo, HTML sanificato e archivi dell'app, con anteprima
      (dry-run), rilevamento duplicati, report e commit transazionale.
- [x] Esportare nota in Markdown, HTML e testo; ZIP Markdown di taccuino o di
      tutte le note con allegati; archivio completo documentato.
- [x] PDF tramite dialog di stampa di sistema e foglio di stile di stampa.
- [ ] Verificare manualmente l'output PDF su Linux (dipende da WebKitGTK e
      dal dialog di stampa).
- [x] Backup manuali, automatici (intervallo e conservazione configurabili),
      pre-migrazione e pre-ripristino; verifica con hash e
      `integrity_check`; ripristino con rollback; copia/ripristino da file
      esterno.
- [x] Controllo d'integrità (database, FTS, allegati) e ricostruzione indice.
- [x] Round-trip verificati da test; procedura di recupero documentata in
      `docs/USER_GUIDE.md`, formati in `docs/ARCHIVE_FORMAT.md`.
- [ ] Verificare manualmente i flussi nella finestra desktop.
- [x] Copie di backup cifrate con password (age v1/scrypt, formato aperto,
      nessun recupero senza password) e ripristino da file `.age` con
      verifica prima della conferma (ADR 0006). I backup interni restano in
      chiaro, protetti dai permessi della cartella dati.
- [ ] Verificare manualmente la decifratura di una copia con la CLI `age`
      (non installata nell'ambiente di sviluppo).
- [x] Rinomina in Greatnote con spostamento automatico della cartella dati
      `dev.gna.notes`; identificatori di formato su disco invariati.

**Accettazione:** import/export supportati e backup ripristinabile verificati
dai test automatici; resta la verifica manuale nella finestra nativa.

## Fase 4 — Task, promemoria e calendario locale

- [x] Migrazione v6: `tasks`, `events`, `reminders` con vincoli, trigger e
      test di upgrade (ADR 0007).
- [x] Attività autonome o collegate a una nota: priorità, scadenza civile,
      completamento, viste Oggi / In ritardo / Prossime / Senza scadenza /
      Tutte aperte / Completate, ricerca parametrizzata, filtro note
      `is:task`.
- [x] Promemoria su note, attività ed eventi: rinvio (+10 min, +1 ora,
      domani alla stessa ora locale), gestione, eliminazione; promemoria persi
      ad app chiusa mostrati al riavvio.
- [x] Notifiche di sistema dal processo Rust (`tauri-plugin-notification`) e
      riquadro persistente nell'app per i promemoria da gestire.
- [x] Calendario mensile e settimanale con eventi locali (con orario o di un
      giorno intero) e scadenze delle attività.
- [x] Attività, eventi e promemoria inclusi nell'archivio completo (campi
      facoltativi di `gna-archive` v1) e nei backup.
- [x] Test su riavvio (riapertura del database), fusi diversi, passaggio
      all'ora legale (orario inesistente) e ritorno all'ora solare (orario
      ripetuto, giorno di 25 ore, rinvio a domani).
- [ ] Verificare manualmente notifiche e flussi nella finestra desktop.
- [ ] Promemoria con app chiusa (servizio in background o timer di sistema):
      non implementato, documentato come limite.
- [ ] Attività e promemoria ricorrenti; vista calendario a griglia oraria.

**Accettazione:** promemoria e scadenze persistono e sono visualizzati
correttamente. Verificato dai test automatici; resta la verifica manuale
delle notifiche nella finestra nativa.

## Interfaccia completa (richiesta 2026-10-14)

- [x] Barra dei menu e menu contestuale sul modello degli screenshot in
      `docs/mockup/` (locali), con azioni reali e scorciatoie (ADR 0009).
- [x] Schede, cronologia avanti/indietro, Vai a…, viste della nota, zoom,
      schermo intero, tema e opzioni della barra laterale persistenti.
- [x] Home, File, Modelli; duplica/copia in, salva come modello, cronologia
      delle revisioni (migrazione v8), informazioni nota, finestra separata.
- [x] Editor con Inserisci, stili, colori, checklist, tabelle, trova e
      sostituisci, comandi "/".
- [x] Test dell'interfaccia automatizzato con backend simulato.
- [ ] Verificare nella finestra nativa WebKitGTK: incolla dal menu, zoom,
      schermo intero, finestra separata, stampa.
- [ ] Importazione Markdown/HTML di checklist e tabelle come nodi nativi.
- [ ] Modelli e revisioni nell'archivio completo `gna-archive`.

## Fase 5 — AI locale/remota e ricerca semantica

- [x] Interfaccia provider (`AiProvider`) e impostazioni privacy disattivate
      per impostazione predefinita (ADR 0010).
- [x] Provider configurabile compatibile OpenAI (locale o remoto con
      consenso e https), prova di connessione, chiave nel portachiavi.
- [x] Chat con recupero delle note e citazioni verificabili; anteprima dei
      dati inviati ai server remoti.
- [x] Modifiche AI con anteprima, confronto e applicazione annullabile; tag
      e attività suggeriti da confermare.
- [x] Embedding e ricerca semantica con indice locale incrementale; test con
      fixture sintetiche e server finti.
- [x] Difese contro la prompt injection e taccuini esclusi.
- [x] Verificato il portachiavi di sistema (Secret Service con KWallet, test
      `system_keyring_round_trip`) e l'indicizzazione con un modello di
      embedding reale (`qwen3-embedding:8b` su Ollama, 2026-10-20).
- [x] Confronto tra versioni di una nota: differenze locali parola per
      parola e spiegazione facoltativa con l'AI (Cronologia nota).
- [x] Cronologia delle richieste AI facoltativa, ricercabile e cancellabile.
- [x] Attività, eventi e promemoria come contesto della chat, con date
      riconosciute nella domanda, fonti citabili ed esclusioni (ADR 0022).
- [ ] Verifica manuale con un modello reale delle domande sul calendario.
- [ ] Motore AI integrato (ADR 0020).
  - [x] llama.cpp incluso nell'installer Windows.
  - [x] Catalogo dei modelli per chat, contesto e visione, scaricabili con
        verifica SHA-256 e ripresa.
  - [x] Processi locali avviati su richiesta e fermati dopo l'inattività.
  - [x] Ricerca dei modelli GGUF su Hugging Face per ruolo, con varianti,
        modelli divisi, proiettori e registro locale (ADR 0021).
  - [x] RAM e schede video rilevate, con l'indicazione di quanto ogni
        modello è adatto e la variante consigliata.
  - [ ] Verifica manuale su Windows: ricerca, download e avvio di un
        modello di Hugging Face; rilevamento delle schede video
        nell'installer.
  - [ ] Verifica manuale su Windows con un modello reale.
- [ ] Trascrizione di riunioni (P3).

**Accettazione:** funzioni AI facoltative, fonti verificabili e nessuna
modifica automatica alle note. Verificata con test automatici e con una prova
reale su server locale (`tests/ai_live.rs`).

## Fase 6 — Integrazioni

- [x] Server MCP locale (stdio, nessuna porta) in sola lettura: ricerca,
      lettura nota, taccuini e tag; disattivato per impostazione
      predefinita, revoca immediata, esclusioni AI rispettate, registro
      senza contenuti (ADR 0014).
- [ ] Strumenti MCP di modifica separati, disattivati per impostazione
      predefinita e con conferma nell'app (solo se richiesti).
- [x] Importazione da Evernote (ENEX), Joplin (JEX) e Obsidian (vault) con
      struttura dei taccuini, tag, allegati e collegamenti (ADR 0011).
- [x] Valutate le integrazioni esterne: scelti solo i calendari (Google,
      Microsoft); posta, file e attività esterne non previste.
- [x] Calendari esterni Google Calendar e Outlook/Microsoft 365: OAuth con
      PKCE nel browser, token nel portachiavi, copia locale offline,
      creazione/modifica/eliminazione con controllo ETag, ricorrenze in sola
      lettura, sincronizzazione ogni 15 minuti, scollegamento con revoca
      (ADR 0015). Test con fornitore e server HTTP finti.
- [x] Registrazioni dell'editore incorporate nella build da variabili
      d'ambiente (collegamento con solo login e consenso); registrazione
      personale nelle impostazioni avanzate (docs/OAUTH_SETUP.md).
- [x] Abbonamenti a calendari iCal in sola lettura (Google, Outlook e
      altri) senza login né registrazioni: indirizzo nel portachiavi,
      ricorrenze, fusi Windows e ora legale, aggiornamento ogni 15 minuti
      (ADR 0016). Verificato con il calendario pubblico delle festività.
- [ ] **Sospeso (2026-10-20, scelta dell'utente):** registrare le app
      dell'editore presso Google e Microsoft non è possibile per mancanza di
      permessi, e la modalità Test di Google richiederebbe di ricollegare
      l'account ogni 7 giorni. Il codice resta testato ma la sezione è
      nascosta finché una build non include gli ID; nessuna prova reale
      eseguita.
- [ ] Sincronizzazione incrementale (`syncToken`/delta) e creazione di
      eventi ricorrenti.
- [x] Documentare permessi, configurazione, revoca e test del server MCP
      (USER_GUIDE, SECURITY, TESTING).
- [ ] Verificare il server MCP su Windows (sottosistema grafico e stdio,
      Fase 8).

**Accettazione:** ogni integrazione è realmente configurabile e testata, senza
mock presentati come operativi.

## Fase 7 — Sincronizzazione

Trasporto scelto: una cartella già sincronizzata (Nextcloud, Syncthing,
Dropbox…), senza server né account (ADR 0017).

- [x] Protocollo versionato: lotti immutabili per dispositivo, registro
      delle modifiche con trigger, coda di uscita nel database, orologi di
      Lamport, consegna causale con dipendenze, tombstone.
- [x] Conflitti senza perdite: vincitore deterministico (convergenza), le
      modifiche prevalgono sulle eliminazioni, versione perdente conservata
      nella cronologia della nota e nell'elenco dei conflitti; salvataggio
      dell'editor protetto da modifiche remote non viste.
- [x] Allegati come blob indirizzati per hash e verificati, con attesa e
      ripresa; tag con lo stesso nome unificati.
- [x] Identità del dispositivo con rilevamento di database copiati.
- [x] Interfaccia: Impostazioni › Sincronizzazione, indicatore di stato,
      ricarica della nota aperta, conflitti da controllare.
- [x] Test: idempotenza, concorrenza, eliminazione contro modifica, lotti
      in disordine, operazioni ostili, allegati in ritardo, database
      copiato; prova su una copia dei dati reali (490 note, 164 allegati).
- [x] Trasporto Dropbox (OAuth PKCE, cartella dell'app, token nel
      portachiavi, revoca) sullo stesso protocollo (ADR 0018).
- [ ] Prova reale su due computer (cartella e Dropbox).
- [ ] Cifratura dei file della cartella con passphrase.
- [ ] Compattazione della cronologia dei lotti (istantanee) per i nuovi
      dispositivi.
- [ ] Fusione automatica del testo per modifiche contemporanee non
      sovrapposte.
- [ ] Trasporto alternativo con server proprio (stesso formato dei lotti).

**Accettazione:** nessuna perdita silenziosa o duplicazione al retry.

## Fase 8 — Porting e rilascio

- [ ] Verificare pacchetti Linux, percorsi XDG e aggiornamenti (incluso che
      il `.desktop` installato abbia `StartupWMClass=dev.greatnote.notes` e
      l'icona compaia su Wayland).
- [x] Configurare CI per frontend e Rust su Linux e Windows.
- [x] Configurare gli installer Windows x64 (NSIS per utente e MSI) e il
      workflow `windows-installer.yml` che li pubblica come artefatti.
- [ ] Verificare l'installer su un PC Windows pulito: installazione,
      avvio, notifiche, portachiavi (Credential Manager), apertura allegati,
      percorsi con spazi e caratteri Unicode, aggiornamento e
      disinstallazione senza perdita dei dati.
- [x] Aggiornamenti in-app firmati (minisign con versione vincolata),
      release in bozza con `latest.json` dal workflow (ADR 0023).
- [ ] Rendere pubblico il repository, configurare i segreti della chiave
      di firma e provare l'aggiornamento reale da una versione all'altra.
- [ ] Aggiornamenti in-app anche per le installazioni Linux (AppImage).
- [ ] Firmare gli installer Windows con un certificato di firma del codice
      (SmartScreen e Smart App Control).
- [x] Hardening: CSP più restrittiva, `cargo audit` nella CI, vulnerabilità
      TLS corrette (RUSTSEC-2026-0381/0382), script con fine riga LF.
- [x] Verifiche backup/restore automatiche: backup da schema precedente
      ripristinato e migrato; ripristino con sincronizzazione attiva senza
      perdita di dati.
- [x] Checklist di rilascio (`docs/RELEASE_CHECKLIST.md`) e controlli
      automatici (`scripts/release-check.sh`).
- [ ] Eseguire la checklist completa su un primo rilascio (passi manuali di
      backup/restore, pacchetti Linux e Windows).

**Accettazione:** pacchetti verificati sulle piattaforme dichiarate con
aggiornamento e ripristino documentati.
