# Sicurezza e privacy

## Modello iniziale

- La base di dati è locale e risiede nella directory dati dell'utente assegnata
  dalla piattaforma Tauri.
- Il frontend non riceve accesso generico al filesystem, alla shell o al
  database.
- I comandi Tauri validano la struttura e la dimensione massima del documento
  JSON prima di salvarlo.
- Le query sono analizzate da un parser limitato e usano statement
  parametrizzati; le espressioni FTS sono quotate prima di passarle a SQLite.
- Gli allegati non sono accessibili dal frontend tramite API filesystem
  generiche. Il core verifica formato tramite magic bytes/UTF-8, estensione,
  dimensione massima di 25 MiB e nomi/percorso generati con UUID.
- Prima di aprire un allegato, il core rifiuta symlink/non-file, controlla la
  dimensione e confronta SHA-256; il plugin opener riceve solo il percorso
  interno verificato.
- Le bozze recuperabili e le note nel cestino restano dati locali nel database
  SQLite; le bozze non vengono applicate senza conferma.
- Le query SQLite usano parametri e lo schema applica foreign key e vincoli.
- Non sono richiesti account, rete, chiavi API o segreti.
- Import/export: i dialog nativi sono aperti dal processo Rust; il frontend
  non ottiene permessi del plugin dialog né percorsi arbitrari. I backup
  interni sono indicati per nome, convalidato con un formato rigido.
- Contenuti importati: Markdown e HTML sono convertiti in nodi ammessi dallo
  schema; script, stili, iframe, gestori di eventi e attributi sono scartati
  e i link sono limitati a `http`, `https` e `mailto`. L'HTML esportato
  codifica il testo e include una CSP `default-src 'none'`.
- Archivi ZIP letti (archivio Greatnote e backup): numero di voci e dimensioni
  limitati con lettura troncata (protezione da zip bomb), nomi interni
  convalidati e mai usati come percorsi, SHA-256 verificato per ogni voce.
- Import di cartelle: profondità e numero di file limitati, file nascosti e
  collegamenti simbolici ignorati; gli allegati del front matter sono
  accettati solo dentro la cartella scelta, senza `..`, percorsi assoluti o
  symlink.
- File esportati e backup sono scritti in file temporanei con permessi 0600 e
  rinominati a scrittura completata; la cartella `backups/` ha permessi 0700.
- La CSP limita le risorse alle origini locali necessarie alla UI.
- In modalità sviluppo, `devCsp` consente lo script inline richiesto dal
  preambolo React Refresh e il WebSocket di Vite esclusivamente su
  `127.0.0.1:1420`. Questa eccezione non si applica alla CSP della build.

- Attività, eventi e promemoria: input convalidati nel core (lunghezze, date,
  fusi IANA risolti dal database del sistema, nessun percorso), query
  parametrizzate e vincoli `CHECK` nello schema. Le notifiche di sistema
  partono solo dal processo Rust; il frontend non ha permessi del plugin di
  notifica. Le notifiche mostrano il titolo dell'elemento sul desktop: chi
  vede lo schermo può leggerlo. I log registrano solo identificativi e
  conteggi, mai titoli.

- Eliminazione definitiva: possibile solo per note già nel cestino, con
  conferma; righe collegate e indice di ricerca sono rimossi in un'unica
  transazione e i file allegati vengono cancellati dopo il commit. Non è una
  cancellazione sicura del supporto: i dati possono restare nello spazio
  libero del database (fino a `VACUUM`), nei file system con copy-on-write o
  SSD e nei backup già creati.

## Threat model iniziale

L'app tratta documenti, query, nomi e contenuti allegati come input non
attendibile. L'IPC è limitato ai comandi applicativi esposti; note e filtri
non autorizzano accessi generici a file o esecuzione di codice. I tipi
ammessi non includono HTML o SVG attivi. File di testo allegati possono
essere indicizzati (massimo 100.000 caratteri) e restano esclusivamente
locali.

## Importazione da altre app

- **ENEX:** XML letto in streaming senza DTD né entità esterne (nessun XXE,
  testato); l'ENML passa dalla stessa conversione HTML sanificata (script,
  iframe e attributi pericolosi scartati).
- **JEX:** l'archivio tar viene letto in memoria; nessun file viene estratto
  su disco, quindi i percorsi nell'archivio non possono scrivere fuori dalla
  cartella dati.
- **Obsidian:** solo file regolari dentro il vault; `..`, percorsi assoluti
  e collegamenti simbolici che escono dal vault vengono rifiutati (testato).
- Gli allegati importati passano dal riconoscimento del tipo per contenuto
  e dai limiti di dimensione come quelli aggiunti a mano.

## Immagini nelle note

- Il documento contiene solo riferimenti ad allegati (`attachmentId`), mai
  URL o percorsi: la validazione Rust rifiuta altri attributi. Immagini
  incollate dal web non generano richieste di rete.
- I byte arrivano dal comando `read_attachment_image`, solo per allegati
  immagine attivi e dopo la verifica di dimensione e SHA-256.
- Lettura del testo: con Tesseract resta sul computer; con un modello AI
  l'immagine è inviata al provider con le stesse regole di consenso e di
  esclusione dei taccuini, entro 5 MB. Il testo letto è trattato come dato
  (istruzioni nell'immagine ignorate) ed è salvato nel database locale.

## Funzioni AI

- **Disattivate per impostazione predefinita.** Nessun dato esce dal
  computer finché l'utente non attiva l'AI e configura un server.
- **Server remoti:** consentiti solo via `https` e con consenso esplicito;
  il backend rifiuta ogni invio senza consenso. Alla prima richiesta
  l'interfaccia mostra cosa verrà inviato (brani delle note, testo o nota);
  l'utente può evitare le conferme successive ("Non chiedermelo più") e
  riattivarle dalle impostazioni. Le fonti inviate restano sempre visibili
  sotto le risposte.
  Nessun reindirizzamento HTTP viene seguito.
- **Claude Code:** avviato senza strumenti (`--tools ""`), in modalità sicura
  (niente hook, plugin, MCP, CLAUDE.md), senza sessione salvata e in una
  cartella temporanea vuota; il testo delle note passa dallo standard
  input, non dagli argomenti. È trattato come server remoto (consenso
  obbligatorio). Vedi ADR 0012.
- **Chiave API:** solo nel portachiavi di sistema (Secret Service /
  Credential Manager); mai nel database, nei backup, nei log o nel frontend.
  Se il portachiavi non è disponibile la chiave non viene salvata.
- **Taccuini esclusi:** le loro note non vengono inviate né indicizzate.
- **Prompt injection:** il contenuto delle note è racchiuso in delimitatori
  che non può falsificare e dichiarato dato non attendibile; il modello non
  ha strumenti né accesso ai dati, e ogni risultato richiede conferma
  dell'utente prima di modificare una nota. Il testo inviato è sempre
  riletto dal database: il frontend passa solo riferimenti.
- **Indice semantico:** contiene vettori e posizioni, non copie del testo;
  segue la nota (eliminazione definitiva, cestino, esclusioni, cambio di
  modello) e si può cancellare dalle impostazioni. È incluso nei backup del
  database come il resto dei dati locali.
- **Cronologia AI:** domande, azioni e risposte restano nel database locale
  (e quindi nei backup), al massimo 1.000 voci; delle fonti si salvano solo
  nota e titolo, non il testo inviato. Si disattiva e si cancella dalle
  impostazioni; le voci di una nota eliminata perdono il collegamento.
- **Log:** gli errori del server sono accorciati e privati dei caratteri di
  controllo; non vengono registrati contenuti delle note né domande.
- **Limiti:** un server remoto riceve il testo inviato e può conservarlo
  secondo le proprie condizioni; un modello può produrre informazioni errate
  (le fonti citate servono a verificarle).

## Sincronizzazione tra dispositivi

- **Nessun segreto nei file, ma nessuna cifratura:** la cartella contiene
  lotti JSON con il contenuto delle note e i blob degli allegati, leggibili
  dal servizio che la ospita. Lo dicono le impostazioni e la guida;
  cifratura con passphrase in roadmap.
- **Dati remoti non attendibili:**
  - ogni operazione viene validata (entità e colonne solo da un elenco
    fisso, chiavi controllate, query parametrizzate, nessuna struttura nei
    valori);
  - un'operazione non valida viene annullata con un savepoint e scartata
    senza lasciare stati parziali;
  - i lotti sono limitati a 64 MB, gli id di dispositivo e libreria devono
    essere UUID.
- **Allegati:**
  - nomi dei file verificati con le regole locali (niente percorsi);
  - contenuto copiato dai blob solo se lo SHA-256 corrisponde;
  - cartella degli allegati creata con permessi 0700.
- **Scritture atomiche** (file temporaneo e rinomina). Ogni dispositivo
  scrive solo nella propria cartella e i lotti sono immutabili.
- **Integrità:**
  - consegna causale, vincitore deterministico e versione perdente
    conservata;
  - un database copiato su due computer viene rilevato (lotti sconosciuti
    nella propria cartella) e la sincronizzazione si ferma.
- **Limiti:**
  - un dispositivo con accesso in scrittura alla cartella può proporre
    modifiche, come qualunque dispositivo dell'utente: non c'è
    autenticazione tra dispositivi oltre all'accesso alla cartella;
  - chi controlla la cartella controlla la libreria sincronizzata.

## Sincronizzazione con Dropbox

- **Accesso:** OAuth 2 con PKCE S256 nel browser di sistema; `state`
  verificato.
  - Il ritorno arriva solo su `http://localhost:41927/`, con listener
    attivo al massimo 5 minuti.
  - Nessun segreto dell'app; l'App key entra nella build e non è nel
    repository.
- **Permessi:** solo la cartella dell'app su Dropbox, con lettura e
  scrittura dei file e lettura dell'account per mostrarne l'indirizzo.
- **Token:**
  - il token di aggiornamento sta nel portachiavi
    (`greatnote-sync/dropbox-refresh`);
  - il token di accesso resta solo in memoria e viene rinnovato prima della
    scadenza;
  - nessun token nei log o nei messaggi d'errore;
  - lo scollegamento revoca il token presso Dropbox.
- **Rete:**
  - solo HTTPS verso le API ufficiali, nessun reindirizzamento;
  - argomenti dei percorsi costruiti solo con caratteri ASCII sicuri;
  - risposte limitate a 80 MB;
  - in caso di 429 o 5xx si riprova con attesa limitata.
- **Dati:** valgono le regole della sincronizzazione (dati remoti non
  attendibili, validazione, hash). I file su Dropbox non sono cifrati.

## Calendari iCal

- **Indirizzo segreto:** resta nel portachiavi di sistema
  (`greatnote-calendar`, `ics-<id>`), mai nel database, nei backup, nei log
  o nei messaggi d'errore.
- **Rete:**
  - solo `https://` (`webcal://` viene convertito);
  - niente credenziali nell'indirizzo, nessun reindirizzamento;
  - risposte al massimo 16 MB;
  - un indirizzo che non restituisce un calendario non viene salvato.
- **Contenuto non attendibile:**
  - titoli e descrizioni sono mostrati come testo;
  - link accettati solo se `https://`;
  - colori validati;
  - ricorrenze limitate a 20.000 occorrenze e 50.000 periodi per regola,
    contro file ostili o enormi.
- **Integrità:** eventi in sola lettura; il database ne impedisce modifiche
  e cestino.

## Calendari esterni (Google, Microsoft)

- **Accesso:** OAuth 2.0 per app native con PKCE S256 e `state` casuale nel
  browser di sistema. Greatnote non vede mai la password.
  - Il ritorno arriva su loopback (`127.0.0.1` o `localhost`, porta casuale)
    e il listener resta attivo solo durante l'accesso, al massimo 5 minuti.
  - Le richieste con `state` diverso vengono ignorate.
- **Permessi minimi:**
  - Google: `calendar.events` e `calendar.calendarlist.readonly`;
  - Microsoft: `Calendars.ReadWrite`, `User.Read`, `offline_access`.
- **Registrazioni incorporate:**
  - gli ID dell'editore entrano nella build da variabili d'ambiente
    (`oauth.env` escluso da git, o segreti della CI), mai nel repository;
  - il segreto dei client desktop di Google è incluso nel programma, come
    previsto da Google per le app native, che lo considera non
    confidenziale: la protezione dell'accesso è data da PKCE e dal
    consenso dell'utente.
- **Segreti:**
  - il token di aggiornamento e l'eventuale segreto di una registrazione
    personale stanno nel portachiavi di sistema;
  - il token di accesso resta solo in memoria;
  - nel database ci sono solo ID client (non segreti), account, calendari
    ed eventi;
  - nessun token finisce nei log o nei messaggi d'errore.
- **Rete:**
  - solo HTTPS verso gli endpoint ufficiali, nessun reindirizzamento;
  - le pagine successive di Graph vengono seguite solo sullo stesso server;
  - risposte limitate a 16 MB.
- **Dati ricevuti:** titoli e descrizioni sono mostrati come testo, mai come
  HTML. I link agli eventi vengono aperti solo se `https://`. Colori dei
  calendari accettati solo nel formato `#rgb`/`#rrggbb`.
- **Integrità:** le modifiche usano `If-Match` sull'ETag e non sovrascrivono
  modifiche fatte altrove. Il database impedisce modifiche locali agli
  eventi esterni.
- **Revoca:**
  - lo scollegamento revoca il token presso Google;
  - per Microsoft, che non lo permette via API, la revoca va fatta
    dall'account;
  - in entrambi i casi il token viene cancellato dal portachiavi.
- **Limite:** con l'app Google in modalità "Test" il token scade dopo
  7 giorni. Chi registra l'app è l'utente stesso, che ne controlla le
  credenziali.

## Server MCP locale

- **Esposizione:** solo stdio. Il server è un processo avviato dal client
  MCP configurato dall'utente; non apre porte e non accetta connessioni di
  rete.
- **Permessi:** database aperto in sola lettura (`SQLITE_OPEN_READ_ONLY` e
  `query_only`) e quattro strumenti di lettura. Disattivato per
  impostazione predefinita; l'impostazione, riletta a ogni chiamata,
  revoca l'accesso immediatamente.
- **Dati esposti:** note attive e archiviate, con taccuino, tag, date e
  testo delle immagini. Mai il cestino né i taccuini esclusi dall'AI, che
  non vengono nemmeno confermati come esistenti. Gli allegati non sono
  accessibili come file.
- **Input:** id validati come UUID (niente percorsi), query limitate a
  500 caratteri e analizzate dal parser della ricerca (nessuna
  concatenazione SQL), righe JSON oltre 1 MB rifiutate.
- **Prompt injection:** il contenuto delle note è dichiarato come dato non
  attendibile e gli strumenti non possono agire sulle note.
- **Registro:** `mcp-audit.jsonl` contiene strumento, esito, conteggio e
  id della nota letta, mai testo di note o ricerche. Il file vive nella
  cartella dati (0700).
- **Limite:** ciò che il client legge può essere inviato al suo fornitore
  di AI secondo le sue condizioni. L'utente lo vede nelle impostazioni
  prima di attivare il server.

## Limiti e mitigazioni pianificate

- **Perdita o corruzione dati:** backup manuali/automatici con hash, backup
  prima delle migrazioni e dei ripristini, ripristino con rollback e
  controllo d'integrità sono disponibili. **I backup interni non sono
  cifrati**: chi accede alla cartella dati legge note e allegati, come per il
  database attivo (si raccomanda la cifratura del disco).
- **Copie di backup fuori dal computer:** "Copia cifrata…" usa age v1 con
  scrypt (libreria `age`, nessuna crittografia proprietaria). Password di
  almeno 10 caratteri, mai salvata né registrata; nessun recupero se persa.
  In ripristino la decifratura avviene in un file temporaneo 0600 nella
  cartella dati, eliminato dopo l'uso o al riavvio successivo. Il work factor
  scrypt accettato è limitato per evitare file ostili che esauriscano
  CPU/memoria. Restano in chiaro: dimensione approssimativa del backup e nome
  del file scelto. La password attraversa l'IPC locale e non viene azzerata
  in memoria in modo garantito (ADR 0006).
- **Contenuti ostili:** il contenuto persistito è JSON strutturato, non HTML
  arbitrario. Allegati testuali non vengono renderizzati come HTML; i formati
  binari supportati sono aperti con il programma di sistema solo dopo
  conferma e controllo d'integrità. L'importazione HTML è sanificata per
  costruzione ed è coperta da test con contenuti ostili.
- **File system:** i nomi utente non determinano il percorso fisico; la
  directory allegati usa permessi restrittivi e rifiuta symlink. La verifica
  di hash precede l'apertura e quella del percorso precede la rimozione.
- **Dipendenze:** revisionare aggiornamenti Rust/npm e controllare gli audit
  disponibili prima dei rilasci.
- **Logging:** non registrare titoli, contenuti, percorsi personali o segreti.
- **Permessi:** mantenere le capability Tauri minime e non esporre origini
  remote.

## Segnalazione vulnerabilità

Fino alla pubblicazione di un canale dedicato, segnalare privatamente al
manutentore del progetto; non allegare note o dati personali alla segnalazione.
