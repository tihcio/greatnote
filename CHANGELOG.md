# Changelog

Le modifiche rilevanti sono registrate per rilascio. Il progetto è in fase
iniziale e non ha ancora release.

## Non rilasciato

- **L'assistente AI conosce i tuoi impegni.** Nella chat si può chiedere
  «Cosa ho domani?», «Quali riunioni ho la prossima settimana?» o «Quando
  devo rinnovare il passaporto?».
  - L'assistente cerca attività, eventi del calendario (anche Google,
    Microsoft e iCal) e promemoria, capendo le date nella domanda («oggi»,
    «venerdì», «28 ottobre», «prossimi 10 giorni»…) e le parole di titoli e
    descrizioni.
  - Questi elementi compaiono tra le fonti come *attività*, *evento* o
    *promemoria*; la citazione apre l'attività o l'evento.
  - Valgono le stesse protezioni delle note: anteprima prima dell'invio a un
    server remoto, nessun elemento collegato a note di taccuini esclusi. Si
    disattiva in Impostazioni › Intelligenza artificiale (ADR 0022).

- **Preparazione ai rilasci.** Nuova
  [checklist di rilascio](./docs/RELEASE_CHECKLIST.md) con lo script
  `scripts/release-check.sh` che esegue tutti i controlli automatici.
  - Corrette due vulnerabilità nella verifica dei certificati TLS usata per
    AI remota, Dropbox e calendari (aggiornati `rustls-webpki` e
    `webpki-roots`). La CI ora controlla anche gli avvisi di sicurezza
    delle dipendenze Rust.
  - Politica di sicurezza dei contenuti più restrittiva (niente plugin,
    URL di base o invio di form).
  - Se si ripristina un backup più vecchio dell'ultima sincronizzazione,
    l'avviso ora lo dice chiaramente; la guida spiega come ricongiungere il
    computer senza perdere note.

- **Modelli AI da Hugging Face, adatti al tuo computer.** Nel motore
  integrato ogni sezione (chat, contesto, visione) ha ora **Cerca altri
  modelli su Hugging Face**.
  - La ricerca mostra i modelli GGUF più scaricati, oppure quelli cercati
    per nome.
  - Per ogni variante si vedono dimensione, memoria stimata e se è adatta
    al computer. La variante consigliata è indicata.
  - In cima compaiono la RAM e le schede video rilevate. Ogni modello,
    anche quelli consigliati, ha un'etichetta: scheda video, processore,
    al limite, troppo grande.
  - I file sono fissati alla revisione corrente e verificati con il loro
    hash. La ricerca invia solo le parole cercate (ADR 0021).

- **Stato della sincronizzazione nella barra laterale.** In basso a sinistra
  non compare più sempre "Archivio locale". Con la sincronizzazione attiva
  si legge "Sincronizzato con Dropbox" (o "con la cartella"); durante la
  sincronizzazione o dopo un errore il messaggio cambia. Passando sopra con
  il mouse si vede l'ora dell'ultima sincronizzazione o l'errore.
- **Collegamento a Dropbox.** Dopo il login nel browser Greatnote torna in
  primo piano e mostra subito l'account collegato. Prima la schermata
  restava su "collegamento in corso" fino alla fine della prima
  sincronizzazione, che ora prosegue in background.
- **Immagini nelle note su Windows.** Ora vengono mostrate (la politica di
  sicurezza dei contenuti non consentiva gli indirizzi `blob:`).

- **Motore AI integrato.** In Impostazioni › Intelligenza artificiale si
  può scegliere il provider **Integrato**: non serve installare Ollama.
  - L'app propone i modelli da scaricare per chat, contesto (ricerca
    semantica) e visione (testo nelle immagini), con dimensione, memoria
    necessaria e licenza. "Scarica i consigliati" li scarica tutti con una
    sola conferma.
  - I download si possono interrompere e riprendere e sono verificati con
    SHA-256. I modelli restano sul computer e non finiscono nei backup.
  - Su Windows l'installer include llama.cpp con accelerazione Vulkan; i
    modelli si avviano solo quando servono e si chiudono dopo 10 minuti di
    inattività (ADR 0020).

- **Installer per Windows.** Ora è possibile compilare per Windows x64 un
  installer `.exe` (NSIS, per l'utente corrente e senza permessi di
  amministratore) e un `.msi`, entrambi in italiano e inglese. Il workflow
  *Windows installer* li compila su GitHub Actions. La CI esegue i
  controlli anche su Windows. Gli installer non sono ancora firmati
  (ADR 0019).
  - Su Windows i report d'importazione mostrano i percorsi con `/`, come su
    Linux (es. `sottocartella/nota.md`), e non più con `\`.

- **Sincronizzazione con Dropbox.** In Impostazioni › Sincronizzazione si
  sceglie **Dropbox** e si preme **Collega Dropbox**: accesso nel browser,
  nessun client da installare.
  - Greatnote usa solo la sua cartella su Dropbox e conserva l'accesso nel
    portachiavi; lo scollegamento lo revoca.
  - Stesso protocollo della cartella sincronizzata: conflitti senza
    perdite, allegati verificati (ADR 0018).

- **Sincronizzazione tra dispositivi.** In Impostazioni › Sincronizzazione
  si sceglie una cartella già sincronizzata (Nextcloud, Syncthing,
  Dropbox…). Non servono account o server.
  - Greatnote scrive solo le modifiche, mai il database, e le applica sugli
    altri computer ogni 2 minuti.
  - Si sincronizzano note, taccuini, spazi, tag, allegati, collegamenti,
    attività, eventi, promemoria, modelli e cestino.
  - Con modifiche contemporanee nessun testo va perso: la versione
    perdente resta nella cronologia della nota e tra le «Modifiche
    contemporanee».
  - Le modifiche prevalgono sulle eliminazioni contemporanee, e un
    database copiato su due computer viene rilevato.
  - Un indicatore in basso mostra l'ultima sincronizzazione (ADR 0017).

- **Calendari iCal (sola lettura).**
  - In Impostazioni › Integrazioni si incolla l'indirizzo iCal di Google
    Calendar, Outlook o altri calendari, senza login né registrazioni. Gli
    eventi compaiono nel calendario di Greatnote con il loro colore, anche
    offline, e si aggiornano ogni 15 minuti.
  - Gli eventi ricorrenti compaiono in ogni occorrenza, con eccezioni e
    spostamenti, e restano corretti al cambio dell'ora legale; sono
    supportati anche i fusi orari usati da Outlook.
  - Sugli eventi si possono aggiungere promemoria. L'indirizzo, segreto,
    resta nel portachiavi (ADR 0016).

- **Calendari esterni sospesi:** la sezione resta nascosta finché una build
  non include le registrazioni dell'app presso Google o Microsoft (oggi non
  disponibili). Il codice e i test restano; nessuna prova reale è stata
  eseguita.
- **Calendari esterni con solo accesso e consenso.**
  - Le registrazioni dell'app presso Google e Microsoft possono essere
    incluse nella build da chi pubblica Greatnote (variabili d'ambiente,
    docs/OAUTH_SETUP.md). L'utente preme «Collega», accede e consente.
  - La registrazione personale resta tra le opzioni avanzate.
- **Calendari esterni: Google Calendar e Outlook/Microsoft 365.**
  - Collegamento da Impostazioni › Integrazioni con accesso nel browser,
    usando app registrate dall'utente; la guida spiega i passaggi.
  - Gli eventi dei calendari scelti compaiono nel calendario di Greatnote,
    con il loro colore e anche offline, e si sincronizzano ogni 15 minuti o
    a richiesta.
  - Gli eventi si creano, modificano ed eliminano da Greatnote. La modifica
    arriva subito al calendario d'origine, e quelle in conflitto con
    modifiche fatte altrove vengono rifiutate.
  - Le occorrenze ricorrenti restano in sola lettura, con il link per
    aprirle nel calendario d'origine.
  - L'accesso resta nel portachiavi; scollegando l'account viene revocato
    (Google) e la copia locale viene rimossa (ADR 0015).

- **Server MCP locale (sola lettura).** In **Impostazioni › Integrazioni**
  si può permettere alle app di AI compatibili con MCP (Claude Code,
  Claude Desktop…) di cercare e leggere le note. È disattivato per
  impostazione predefinita, l'app di AI avvia `greatnote --mcp` e
  comunica tramite stdin/stdout, senza porte di rete. Il database è aperto
  in sola lettura. Strumenti: `search_notes`, `read_note`,
  `list_notebooks`, `list_tags`. Cestino e taccuini esclusi dall'AI non
  sono mai accessibili; la revoca vale subito. Il registro delle attività
  non contiene testo di note o ricerche. Le impostazioni mostrano il
  comando per Claude Code e la configurazione JSON (ADR 0014).

- **Confronto tra versioni.** In **Cronologia nota** ogni versione salvata
  ha tre schede: **Versione** (il testo di allora), **Differenze con
  l'attuale** (aggiunte in verde e rimozioni in rosso, calcolate in locale
  senza AI) e **Spiegazione AI**. Quest'ultima, su richiesta, riassume cosa
  è stato aggiunto, rimosso o modificato. Valgono le regole delle altre
  funzioni AI: taccuini esclusi, conferma per i server remoti, testi
  delimitati come dati non attendibili, richiesta nella cronologia AI.

- Corretto: sui bordi delle colonne comparivano barre verticali verdi che al
  passaggio del mouse sembravano sdoppiarsi. Erano le barre di scorrimento:
  WebKitGTK dà la precedenza a `scrollbar-color` e disegnava la barra nativa
  con un colore verdastro accanto alla linea del bordo trascinabile. Ora le
  barre sono sottili e neutre, e nella barra laterale e nell'elenco delle
  note compaiono solo al passaggio del mouse. Il bordo non prende più il
  focus con il clic e il trascinamento termina correttamente anche se il
  puntatore viene perso.

- Verificati su sistema reale il portachiavi (KWallet tramite Secret
  Service) e l'indicizzazione semantica con un modello di embedding locale.

- **Colonne ridimensionabili:** trascinando il bordo tra barra laterale ed
  elenco delle note, o tra elenco ed editor, si cambia la larghezza delle
  colonne (salvata nelle preferenze; doppio clic per tornare alla larghezza
  predefinita; dalla tastiera con le frecce). L'editor mantiene almeno
  380 px.

- **Immagini ridimensionabili e annotabili.** Nella nota ogni immagine ha
  una maniglia per ridimensionarla e una barra con Piccola, Media, Grande,
  Originale e **Annota**. L'editor delle annotazioni offre penna,
  evidenziatore, freccia, rettangolo, cerchio, note di testo, gomma, otto
  colori, tre spessori e annulla/ripeti. Le annotazioni sono vettoriali e
  salvate nella nota (l'immagine originale non cambia, si annullano con
  Ctrl+Z, seguono copie e revisioni). Le note scritte sulle immagini sono
  ricercabili, le legge l'assistente AI e compaiono nelle esportazioni.

- Corretto: il testo letto dalle immagini era indicizzato ma nelle ricerche
  sembrava assente. Le parole ora si cercano anche per prefisso (`euron`
  trova «EURONICS», i risultati compaiono mentre si scrive; le frasi tra
  virgolette restano esatte) e l'anteprima viene presa dal campo che
  corrisponde, quindi mostra il testo dell'immagine con la parola evidenziata.

- Corretto: la lettura del testo con un modello locale con visione (es.
  `qwen3-vl:8b` con contesto di 4096 token) falliva con "exceeds the
  available context size" perché l'immagine era inviata a risoluzione
  piena. Ora viene ridotta (1024 px sul lato lungo, configurabile) e, se il
  server segnala ancora contesto insufficiente, Greatnote riprova da solo
  più piccola; Tesseract continua a usare la risoluzione piena.

- **Immagini nel testo delle note** (ADR 0013): le immagini allegate,
  trascinate o incollate compaiono nella nota; quelle già allegate si
  inseriscono dal pannello allegati; le immagini importate da Evernote,
  Joplin e Obsidian tornano al loro posto nel testo. Il documento accetta
  solo riferimenti ad allegati (mai URL), e copie, archivi ed esportazione
  Markdown li gestiscono.
- **Testo nelle immagini** (migrazione v11): "Leggi il testo" sulle
  immagini, lettura automatica all'aggiunta (opzione) e lettura di tutte le
  immagini in background, con un modello AI con visione (anche Claude Code)
  o con Tesseract locale. Il testo letto è ricercabile e l'assistente AI lo
  usa per rispondere.

- Server remoti e Claude Code: la conferma con l'elenco di ciò che verrà
  inviato compare solo alla prima richiesta, con "Non chiedermelo più"
  (spuntato). Le fonti usate restano visibili sotto ogni risposta; la
  conferma per ogni invio si riattiva nelle impostazioni AI.

- **Claude Code come provider AI** (ADR 0012): in Impostazioni ›
  Intelligenza artificiale si può scegliere Claude Code installato sul
  computer, che usa l'account già configurato. Viene avviato senza
  strumenti, plugin, hook o MCP, in una cartella temporanea vuota, con il
  testo delle note solo sullo standard input. "Verifica Claude Code"
  controlla versione e accesso senza fare richieste al modello. Richiede il
  consenso all'invio ad Anthropic; non offre la ricerca per significato.

- **Cronologia delle richieste AI** (migrazione v10): scheda **Cronologia**
  nell'Assistente con ricerca, dettaglio di risposta e fonti, copia,
  apertura della nota, **Riprendi la conversazione**, eliminazione singola o
  totale. Registra chat, modifiche, tag, attività e ricerche (anche quelle
  non riuscite, non quelle annullate), al massimo 1.000 voci, solo sul
  dispositivo; delle note citate conserva solo il titolo. Si disattiva e
  si cancella da Impostazioni › Intelligenza artificiale.
- **Cerca per significato:** senza modello di embedding il campo non era
  utilizzabile e non spiegava perché. Ora la scheda spiega a cosa serve e
  cosa configurare, e avvisa se l'indice è vuoto.

- Corretto: aprire una nota dall'elenco "Recenti" della barra laterale la
  spostava in cima, cambiando l'ordine sotto il puntatore. Le note già in
  elenco ora restano al loro posto; solo quelle nuove entrano in cima.

- Corretto: il dialog dei risultati AI (riassumi, traduci, …) era troppo
  stretto e il testo andava scorso. Ora è largo fino a 1040 px e alto quanto
  il contenuto (fino al 94% della finestra), con i pulsanti sempre visibili.

- Nuova grafica: palette neutra con accento verde di Greatnote (tema chiaro e
  scuro), icone a linee originali, pulsante **Nota** arrotondato, elenco
  delle note a schede (o a righe), barra superiore dell'editor con percorso
  «Taccuino › Titolo» (cliccabile per spostare la nota), barra di
  formattazione a tutta larghezza, testi più leggibili in tutte le viste.
  I tag non occupano più lo spazio sopra l'editor: sono nella barra in basso
  con suggerimenti, creazione con Invio e rimozione con `×`, accanto al
  promemoria e allo stato del salvataggio.

- Importazione da **Evernote** (file ENEX), **Joplin** (archivio JEX) e
  **Obsidian** (cartella del vault) (ADR 0011): taccuini d'origine
  mantenuti (anche annidati, facoltativamente in un nuovo spazio), tag,
  date, allegati incorporati, collegamenti tra note, caselle di Evernote,
  front matter e `#tag` di Obsidian. Anteprima con conteggi e avvisi,
  duplicati riconosciuti, commit in un'unica transazione.

- Corretto: le impostazioni AI restavano su "Caricamento…" nei database
  portati alla v9 da una versione di sviluppo con uno schema provvisorio
  dell'indice semantico. All'apertura lo schema viene verificato e l'indice
  (solo dati ricalcolabili) ricreato; il modulo mostra gli errori di
  caricamento con "Riprova".

- Assistente AI con Open WebUI: preset `http://localhost:3000/api`, la prova
  di connessione usa la chiave appena digitata, "Salva le impostazioni AI"
  salva anche la chiave nel portachiavi, messaggi chiari per indirizzo
  sbagliato (pagina web al posto dell'API) e chiave mancante.

- Fase 5 — funzioni AI facoltative (ADR 0010), disattivate per
  impostazione predefinita: provider compatibili OpenAI (Ollama, LM Studio,
  llama.cpp, servizi remoti via https con consenso), chiave API nel
  portachiavi di sistema, vista **Assistente AI** con chat che cita le note
  usate e ricerca per significato, modifiche AI con anteprima e confronto
  (correggi, riscrivi, riassumi, espandi, traduci, elenco, decisioni e
  azioni, formattazione), tag e attività suggeriti da confermare, taccuini
  esclusi, indice semantico locale (migrazione v9) con aggiornamento
  manuale o automatico. Test con server finti e prova facoltativa con un
  server locale reale.

- Licenza GPL-3.0-only (`LICENSE`). Il logo sorgente è ora
  `docs/branding/logo.png`; gli screenshot di riferimento in `docs/mockup/` e
  `.vscode/` restano locali e non sono pubblicati.

- Corretto: la barra laterale poteva restare nascosta senza un modo evidente
  per riaverla (vista "Concentrazione" o "Nascondi barra laterale"). Nuovo
  pulsante ⟨⟨/⟩⟩ sempre visibile nella barra delle schede; uscendo dalle note
  la vista torna normale; "Mostra barra laterale" esce anche dalla
  concentrazione.

- Corretto: dopo aver salvato le modifiche la finestra non si chiudeva (in
  particolare la finestra separata di una nota). Tauri 2 chiude le finestre
  con un gestore di chiusura tramite `destroy()`, che mancava tra i permessi;
  ora la finestra viene chiusa direttamente dopo il salvataggio e un
  eventuale errore viene mostrato.

- **Barra dei menu** File, Modifica, Visualizza, Nota, Finestra, Strumenti e
  Aiuto sul modello delle app di note desktop, con scorciatoie da tastiera e
  menu contestuale delle note; ogni voce esegue un'azione reale (ADR 0009).
- **Schede** con cronologia avanti/indietro, **Vai a…** (Ctrl+Q), viste della
  nota (normale, editor ampliato, concentrazione), zoom, schermo intero,
  tema chiaro/scuro/sistema e opzioni della barra laterale, salvati nel
  database.
- Nuove sezioni **Home**, **File** e **Modelli** (predefiniti e dell'utente,
  anche nel cestino); "Preferite" diventa **Scorciatoie**.
- Note: duplica, copia in un altro taccuino (con tag e allegati), salva come
  modello, copia come Markdown/testo, copia link e incolla collegamento,
  informazioni nota, **cronologia delle revisioni** con ripristino, apertura
  in una **finestra separata**.
- Editor: barra "Inserisci", stili, carattere, dimensione, colore,
  evidenziazione, **checklist**, **tabelle**, trova e sostituisci, comandi
  rapidi "/"; esportazione Markdown/HTML/testo di checklist e tabelle.
- Attività: nuovo dialog con scadenza e promemoria rapidi, priorità e
  **contrassegno** (vista "Importanti").
- Migrazione v8 (revisioni, modelli, contrassegno delle attività).
- Test dell'interfaccia `npm run test:ui` (Firefox headless, backend
  simulato).

- Icona corretta nella barra delle applicazioni su Wayland: la finestra usa
  l'app_id `dev.greatnote.notes` e i pacchetti installano un `.desktop`
  corrispondente; per lo sviluppo c'è `scripts/install-dev-desktop-entry.sh`.
  Anche le notifiche indicano la voce `.desktop` dell'app. Si può aprire una
  sola istanza alla volta.

- Nuovo logo (da `docs/branding/logo.png`, ritagliato e
  ridimensionato) nella barra laterale e come icona dell'app. La scritta
  Greatnote usa Manrope incluso nell'app (nessuna richiesta di rete): "Great"
  in Bold, "note" in SemiBold.

- **Cestino per tutti gli elementi eliminabili**: spazi, taccuini, tag,
  attività ed eventi vanno nel cestino invece di essere eliminati subito.
  Nel Cestino, la sezione "Altri elementi" permette di ripristinarli (con i
  collegamenti a note e taccuini) o eliminarli definitivamente; **Svuota
  cestino** elimina tutto. Migrazione v7 (ADR 0008).

- Le conferme usano ora un dialog dell'app invece di `window.confirm`, non
  affidabile in WebKitGTK: titolo, testo e pulsanti espliciti; per le azioni
  irreversibili (eliminazione definitiva, svuota cestino, ripristino backup,
  eliminazioni) pulsante rosso e focus iniziale su Annulla. Esc o clic fuori
  annullano; per il recupero di una bozza conservano le modifiche.

- Cestino: **Elimina definitivamente** una nota e **Svuota cestino**, con
  conferma. Vengono rimossi anche file allegati, collegamenti, tag, bozze,
  promemoria della nota e indice di ricerca; attività ed eventi collegati
  restano senza nota. Possibile solo per note già nel cestino.

- Promemoria: nuova sezione **Gestiti** con lo storico dei promemoria fatti,
  archiviati o chiusi completando un'attività (orario previsto e momento della
  gestione), con apertura dell'elemento, eliminazione e svuotamento dello
  storico.

- Corretto: aggiungere un promemoria a un'attività o a un evento salvava e
  chiudeva il dialog (form annidato) senza creare il promemoria.
- Corretto: su Linux non si poteva scegliere l'ora (WebKitGTK non ha un
  selettore per `datetime-local`). Promemoria ed eventi usano ora data, ore e
  minuti separati; spostando l'inizio di un evento la fine ne conserva la
  durata.
- Promemoria arrivati in ritardo indicati come "Scaduto alle …" e "perso
  mentre l'app era chiusa" (soglia 1 minuto); all'avvio una notifica riepiloga
  i promemoria ancora da gestire. Su Linux le notifiche sono inviate in modo
  sincrono, restano finché non vengono chiuse e gli errori di consegna sono
  registrati nei log.

- Corretto: selezionare una nota salvava sempre quella lasciata (anche senza
  modifiche) e ricaricava la lista, che si riordinava sotto il cursore. Ora
  si salva solo se ci sono modifiche, un salvataggio identico non cambia la
  data di modifica e la lista viene aggiornata sul posto.
- Corretto: il pannello della nota non scorreva quando il contenuto superava
  l'altezza della finestra.

- **Attività**: autonome o collegate a una nota, con priorità, scadenza,
  completamento e viste Oggi, In ritardo, Prossime, Senza scadenza, Tutte
  aperte e Completate; ricerca; contatore nella barra laterale; elenco delle
  attività nella nota aperta. Nuovo filtro di ricerca `is:task`.
- **Promemoria** su note, attività ed eventi con notifica di sistema,
  riquadro persistente per quelli da gestire, rinvio (+10 min, +1 ora,
  domani) e recupero dei promemoria persi mentre l'app era chiusa.
- **Calendario** locale mensile e settimanale con eventi (con orario o di un
  giorno intero) e scadenze delle attività.
- Gestione esplicita di fusi orari e ora legale (libreria `jiff`).
- Migrazione v6 (`tasks`, `events`, `reminders`); l'archivio completo
  include attività, eventi e promemoria mantenendo la compatibilità.

- L'app si chiama ora **Greatnote** (identificatore `dev.greatnote.notes`).
  Database, allegati e backup della cartella `dev.gna.notes` vengono spostati
  automaticamente al primo avvio, anche se nel frattempo è stato creato un
  database nuovo ancora vuoto (conservato in `.replaced-<id>/`); se entrambe
  le cartelle contengono note non viene modificato nulla.
- Copie di backup cifrate con password nel formato aperto age (scrypt), con
  ripristino da file `.age`: decifratura in copia temporanea privata,
  verifica completa e conferma prima del ripristino.
- Gli allegati usano la cartella dati effettiva del database invece di
  ricalcolarla dall'identificatore dell'app.

- Importazione con anteprima e rilevamento duplicati di file Markdown,
  cartelle Markdown (front matter con titolo, date, tag e allegati), testo,
  HTML sanificato e archivi dell'app; commit in un'unica transazione con report.
- Esportazione di una nota in Markdown, HTML o testo e PDF tramite il dialog
  di stampa; ZIP Markdown di un taccuino o di tutte le note con allegati;
  archivio completo `gna-archive` v1 documentato.
- Backup ZIP verificabili (manuali, automatici all'avvio, prima delle
  migrazioni e prima dei ripristini), ripristino con rollback, copia e
  ripristino da file esterno, controllo d'integrità e ricostruzione
  dell'indice di ricerca, nel nuovo dialog "Dati e backup".
- Migrazione v5: impostazioni locali, provenienza delle note importate e
  reindicizzazione delle note escluse dalla ricostruzione FTS v3→v4.
- Corretto: una nota nel cestino al momento della migrazione v3→v4 non era
  più ricercabile dopo il ripristino.
- Migrazione automatica v3→v4 per database esistenti: aggiunge in modo
  compatibile la colonna usata per indicizzare gli allegati e ripristina
  l'aggiornamento della ricerca durante le modifiche alle note; ricostruisce
  inoltre gli indici FTS v3 privi della colonna allegati.
- Migrazione v3 con indice FTS5, filtri di ricerca validati, visite recenti,
  allegati e collegamenti interni persistenti.
- Ricerca locale con frasi, filtri tipizzati, anteprime/evidenziazione e viste
  server-side per tutte le note, preferite, recenti, archivio e cestino.
- Preferite e archivio con azioni nell'editor; scorciatoia `Ctrl/Cmd+Shift+N`
  per creare un taccuino.
- Allegati locali (testo, PNG, JPEG, GIF, PDF) con limite 25 MiB, rilevamento
  MIME, deduplicazione SHA-256, protezione da path traversal/symlink e
  verifica d'integrità prima dell'apertura.
- Collegamenti locali tra note, con navigazione e rimozione indipendenti dal
  contenuto rich-text.
- Icona SVG dedicata e riconoscibile per la navigazione del cestino.
- Le nuove note create nel contesto di un taccuino vengono assegnate
  direttamente a quel taccuino e restano nella relativa vista.
- CRUD locale di spazi, taccuini e tag, con selezione e filtri dalla barra
  laterale e assegnazioni persistenti alle note.
- Cestino con eliminazione logica e ripristino; eliminare contenitori
  scollega le note invece di rimuoverle.
- Bozze persistenti in SQLite, ripristinabili o scartabili dopo un arresto
  anomalo senza sovrascrivere la versione salvata prima della conferma.
- Migrazione automatica da schema SQLite v1 a v2.
- Test SQLite per relazioni, rollback, eliminazione sicura e riapertura del
  database.
- Ricognizione della toolchain Linux e documentazione iniziale.
- Impostazione architettura Tauri 2 + React/TypeScript e core Rust locale.
- UI indipendente dal desktop environment; nessuna API KDE.
- CRUD locale delle note con editor strutturato e salvataggio automatico.
