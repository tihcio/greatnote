# Changelog

Le modifiche rilevanti sono registrate per rilascio.

## Non rilasciato

- **Android: condividere una pagina web salva l'articolo.** Prima la nota
  conteneva solo titolo e indirizzo; ora Greatnote scarica la pagina e ne
  tiene il contenuto principale (testo, titoli, elenchi, tabelle, link),
  con il link alla fonte in cima. Se la pagina non è leggibile resta il
  link con un avviso.
- **Nuova nota da pagina web** (menu File) e **Importa la pagina in una
  nuova nota** (menu contestuale di un link): lo stesso anche sul computer.
- **MCP: cestino e riscrittura con conferma.** Un nuovo permesso nelle
  Integrazioni lascia che l'app di AI *chieda* di spostare una nota nel
  cestino o di riscriverne il testo. Greatnote avvisa con una notifica e
  mostra l'anteprima delle differenze: la nota cambia solo se confermi, e
  prima di una riscrittura la versione attuale va nella cronologia.
- **Sincronizzazione cifrata con passphrase.** Da Impostazioni ›
  Sincronizzazione › Cifratura i file nella cartella o su Dropbox vengono
  cifrati sul dispositivo: chi li ospita non legge più note, titoli,
  allegati né nomi dei dispositivi. Una sincronizzazione già in uso viene
  convertita senza perdere nulla; gli altri dispositivi chiedono la
  passphrase una volta, poi la chiave resta nel portachiavi. La passphrase
  si può cambiare senza disturbare gli altri dispositivi. Le versioni
  precedenti si fermano con un avviso invece di scrivere in chiaro.
- **Nuovi dispositivi più rapidi da sincronizzare.** Quando la storia è
  lunga un dispositivo salva nella cartella un'istantanea della libreria
  (al massimo una volta al giorno). Chi si aggiunge parte da lì invece di
  rileggere ogni modifica, che prima poteva richiedere ore.
- **Modifiche contemporanee unite da sole.** Se la stessa nota viene
  modificata su due dispositivi in parti diverse (paragrafi, voci di un
  elenco, titolo e testo), la sincronizzazione unisce le due modifiche
  invece di tenerne una e mettere l'altra nella cronologia. Lo stesso
  paragrafo cambiato su entrambi resta un conflitto da controllare, come
  prima.
- Test dell'interfaccia: il clic sulla griglia oraria evita la barra di
  scorrimento (falliva a certe ore del giorno).

## 0.2.15 — 2026-11-25

- **Android: condividere verso Greatnote.** Dal pannello «Condividi» di
  un'altra app testo, link, immagini, PDF e file di testo (anche più
  insieme) diventano una nota, che si apre subito.

- **Più veloce con archivi grandi.** Con 30.000 note il controllo
  d'integrità passa da 149 secondi a mezzo secondo, e salvare o creare una
  nota da 9 ms a 0,16 ms (il tempo non cresce più con il numero di note);
  anche le importazioni grandi ne beneficiano. Il database passa alla
  versione 17. Nuova prova ripetibile delle prestazioni
  (`tests/performance.rs`).
- Assegnare due volte lo stesso tag a una nota non dà più un errore.

- **Ricerca con AND, OR, NOT e parentesi.** Combinano parole, frasi e
  filtri (`progetto OR piano`, `budget NOT bozza`,
  `(tag:lavoro OR tag:casa) is:pinned`); senza operatori la ricerca resta
  quella di prima, con pertinenza ed evidenziazione.
- **Ricerca nelle attività senza distinzione di accenti:** «citta» trova
  «Città», «CITTÀ» trova «città».

## 0.2.14 — 2026-11-25

- **Backup: le copie di sicurezza non si accumulano più.** Le copie create
  prima di ogni aggiornamento del database e prima di un ripristino non
  venivano mai eliminate (il numero di backup conservati valeva solo per
  quelli automatici): ora di ciascun tipo restano le 3 più recenti. Una
  soglia più bassa per i backup automatici si applica subito al
  salvataggio. I backup manuali restano finché non li elimini.
- **Sicurezza:** dipendenze `url` e `cookie` aggiornate (RUSTSEC-2026-0592,
  RUSTSEC-2026-0594).

## 0.2.13 — 2026-11-25

- **Promemoria anche a finestra chiusa sul computer.** In Impostazioni ›
  Generali › In background Greatnote può restare attivo nell'area di
  notifica quando chiudi la finestra, e avviarsi all'accesso con la finestra
  nascosta. Aprire di nuovo Greatnote mostra la finestra già attiva invece
  di avviarne una seconda copia. Su Linux i pacchetti richiedono ora
  `libayatana-appindicator`.

- **Eventi che si ripetono.** Nel dialog dell'evento c'è **Ripeti** (ogni
  giorno, settimana, mese o anno, con intervallo, giorni e data di fine).
  Le occorrenze mantengono l'ora locale con il cambio dell'ora; una singola
  occorrenza si può modificare o eliminare da sola, con le successive o con
  tutta la serie.
- **Promemoria che si ripetono.** I promemoria di una nota possono
  ripetersi ogni giorno, settimana, mese o anno; quelli di un evento che si
  ripete seguono l'evento con lo stesso anticipo.

- **Tabelle e checklist importate come tali.** Importando Markdown, HTML o
  note da Evernote, Joplin e Obsidian, tabelle e checklist diventano
  tabelle e checklist modificabili (prima erano righe di testo con «|» o
  simboli ☑/☐). Restano celle unite, intestazioni e stato delle spunte.

- **L'archivio completo include modelli e cronologia delle revisioni.**
  Esportando e reimportando l'archivio completo si ritrovano i modelli
  creati e le versioni precedenti di ogni nota; gli archivi delle versioni
  precedenti restano importabili.

## 0.2.12 — 2026-11-20

- **Tabelle: righe e colonne a scelta e comandi a portata di mano.**
  «Inserisci tabella» apre un dialog con una griglia e due campi per
  scegliere righe e colonne (prima la tabella era sempre 3×3) e se usare
  la prima riga come intestazione. Con il cursore in una tabella compare
  una barra per inserire ed eliminare righe e colonne, unire e dividere
  celle, svuotarle, impostare le intestazioni ed eliminare la tabella. Il
  menu contestuale mostra i comandi della tabella in cima. Le celle unite
  sono esportate correttamente in HTML (`colspan`/`rowspan`), Markdown e
  testo semplice.

- **Esporta come PDF senza il dialog della stampante.** «Esporta nota come
  PDF…» e la voce **PDF…** del menu Esporta chiedono dove salvare il file e
  lo generano direttamente, con lo stesso aspetto della stampa. **Stampa
  nota…** (`Ctrl+P`) apre ancora il dialog di stampa.

- **Impostazioni: la sezione Dati contiene direttamente importazione,
  esportazione, backup e integrità.** Prima mostrava solo un pulsante che
  chiudeva le impostazioni e apriva una finestra separata. Anche le voci dei
  menu (Dati e backup, Importa, Verifica integrità) aprono ora le
  impostazioni sulla sezione Dati; durante un'operazione in corso le
  impostazioni non si chiudono e non si cambia sezione.

## 0.2.11 — 2026-11-18

- **Android: promemoria che non arrivavano.** Il permesso delle notifiche
  veniva chiesto solo aggiungendo un promemoria dal pannello della nota: i
  promemoria delle attività e quelli arrivati dalla sincronizzazione non
  comparivano. Ora l'app lo chiede anche lì e all'avvio, se ci sono
  promemoria programmati. Su alcuni telefoni (provato su un vivo) chiudere
  l'app dalle app recenti cancellava i promemoria programmati: la vista
  Promemoria propone **Consenti in background**, che chiede ad Android di
  non ottimizzare la batteria per Greatnote.

- **Sincronizzazione: nome del dispositivo e gestione dell'elenco.** Senza
  un nome scelto si usa il nome host del computer (su Windows prima
  risultava «Computer») o il nome del telefono su Android (prima
  «Android»). Il nome si può cambiare in qualunque momento con **Salva il
  nome**: con Dropbox collegato il campo era modificabile ma il nome non
  veniva mai salvato. L'elenco **Dispositivi** mostra anche questo
  dispositivo, e quelli non più usati si tolgono con **Rimuovi
  dall'elenco**, su tutti i dispositivi e senza cancellare dati.

- **CI: avviso su `picomatch` accettato.** `npm audit` bloccava la CI per
  un ReDoS in `picomatch` (GHSA-fc5q-39xc-vjpf), senza versione corretta.
  Lo usano solo Vite e i suoi strumenti durante la compilazione: non entra
  nell'app. `scripts/npm-audit.mjs` lo elenca con la motivazione e fa
  fallire la CI per ogni altro avviso.

## 0.2.10 — 2026-11-18

- **Aggiornamenti dall'app anche su Android.** Se su GitHub è pubblicata
  una versione più recente, l'app scarica l'APK firmato della release e
  chiede se installarlo; «Aggiorna» salva le note aperte e apre
  l'installatore di Android. Android accetta l'aggiornamento solo se è
  firmato con la stessa chiave dell'app installata. Impostazioni ›
  Aggiornamenti è disponibile anche sul telefono. Le release includono ora
  `Greatnote.arm64.apk`.

## 0.2.9 — 2026-11-17

- **Windows: pacchetto per il Microsoft Store.** Il workflow crea anche un
  pacchetto MSIX da pubblicare sullo Store, che lo firma: niente più
  blocchi di Smart App Control e SmartScreen. Usa la stessa cartella dati
  degli installer di GitHub e non la cancella disinstallando. Nella versione
  Store gli aggiornamenti arrivano dallo Store, le notifiche dei promemoria
  usano l'identità del pacchetto e il server MCP si avvia con l'alias
  `greatnote.exe` (docs/MICROSOFT_STORE.md, ADR 0029).
- **Android: icona di Greatnote.** L'app mostrava ancora l'icona
  predefinita di Tauri. Ora usa il logo, anche come icona adattiva con il
  logo dentro l'area sicura della maschera.
- **Firma con SignPath non attiva.** La richiesta a SignPath Foundation è
  stata respinta (progetto ancora poco conosciuto): il workflow resta
  pronto, ma README e installer non dichiarano più una firma che non c'è.

## 0.2.8 — 2026-11-17

- **Windows: modifiche ravvicinate non più ignorate.** L'orologio di
  SQLite su Windows avanza a scatti di circa 15 ms: due modifiche alla
  stessa nota entro lo stesso scatto ricevevano la stessa data di modifica,
  quindi l'indice AI non reindicizzava la nota e il controllo delle
  versioni della sincronizzazione non vedeva il cambiamento. Ora la data
  di modifica di una nota avanza sempre, almeno di 1 ms.

- **Windows: firma del codice con SignPath Foundation.** Le release
  possono firmare con Authenticode `greatnote-desktop.exe`, l'installer NSIS e i
  pacchetti MSI, così SmartScreen e Smart App Control non bloccano più
  l'avvio. La firma si attiva quando il progetto SignPath è configurato
  (docs/CODE_SIGNING.md, ADR 0028); le firme degli aggiornamenti in-app
  vengono rigenerate sui file firmati.
- **Installer: pagina di licenza e privacy.** NSIS e MSI mostrano la
  licenza GPL-3.0 e quali connessioni di rete fa l'app senza una richiesta
  esplicita (il controllo degli aggiornamenti, disattivabile).

## 0.2.7 — 2026-11-17

- **Spostare una nota in uno spazio nuovo.** Una nota sta sempre in un
  taccuino, quindi uno spazio appena creato, senza taccuini, non compariva
  in **Sposta in…**. Ora il selettore raggruppa i taccuini per spazio,
  mostra anche gli spazi vuoti e per ognuno offre **Nuovo taccuino in
  «…»**, che crea il taccuino e ci sposta (o copia) la nota.

- **Trascinare le note sulla barra laterale (Windows e Linux).** Una nota
  dell'elenco si può trascinare su un taccuino (o su Senza taccuino) per
  spostarla, su uno spazio per scegliere o creare uno dei suoi taccuini, su
  un tag per aggiungerlo, su Scorciatoie, Archivio o Cestino. Il
  trascinamento usa gli eventi del puntatore, così funziona allo stesso
  modo su Windows (dove il drag and drop HTML5 è bloccato dalla gestione
  dei file trascinati) e su Linux.

## 0.2.6 — 2026-11-16

- **AI integrata: la risposta non resta più nascosta dietro il
  ragionamento.** Con Gemma 4 l'assistente mostrava un lungo ragionamento
  in inglese («Here's a thinking process…») e la risposta arrivava solo
  dopo, spesso oltre il tempo massimo. Il motore ora concede al modello un
  ragionamento breve nel suo canale separato, che non viene mostrato:
  compare solo la risposta, con pochi secondi di attesa in più.

- **Messaggi d'errore in inglese.** Con il sistema in inglese gli errori
  prodotti dal motore dell'app comparivano in italiano. Ora circa 420
  messaggi (note, ricerca, allegati, backup, importazione, calendari,
  sincronizzazione, AI, aggiornamenti) seguono la lingua dell'interfaccia,
  anche su Android (ADR 0027). Anche i titoli predefiniti delle note
  importate («Nota senza titolo») seguono la lingua. Il riconoscimento
  dell'accesso Dropbox scaduto non dipende più dal testo dell'errore.

- **Sincronizzazione: niente falsi conflitti su un dispositivo nuovo.** Un
  dispositivo che entrava in una libreria già usata da altri due
  riapplicava la loro storia e segnalava come «modifiche contemporanee» i
  vecchi conflitti tra quei dispositivi, anche senza aver modificato nulla.
  Ora la storia precedente all'ingresso non genera avvisi; i conflitti che
  riguardano le modifiche fatte sul dispositivo restano segnalati.

- **Ricerca rapida mentre scrivi.** La ricerca di `Ctrl+K` trova le note
  anche con una parola scritta a metà («funz» trova «funziona»); prima
  cercava solo la frase esatta. Con punteggiatura o simboli resta la frase
  esatta.

- **Android: lavori in corso.** Il progetto si compila per Android e ha una
  disposizione per telefono con cui creare e modificare note, allegare
  file, fare backup e ripristini (anche di backup creati sul desktop) e
  importare ed esportare; i promemoria arrivano come notifiche di sistema
  anche ad app chiusa. Sincronizzazione Dropbox, calendari iCal,
  condivisione verso altre app e AI con un server compatibile OpenAI usano
  lo stesso codice del desktop. Su mobile «Trova e sostituisci», «Testo
  cifrato» e «Incolla senza formattazione» sono nel menu «Altro» della
  barra; dopo una chiusura forzata l'app riapre la nota che era aperta.
  La build di rilascio si firma con una chiave fuori dal repository.
  Provato su emulatore e su un Pixel 7 Pro; non esiste ancora una versione
  Android distribuita. Stato e piano in `docs/ANDROID_PORTING.md`.
  Nessun cambiamento per Linux e Windows.

## 0.2.5 — 2026-11-11

- **Calendario con griglia oraria.** Le viste **Settimana** e la nuova
  vista **Giorno** mostrano le ore: un clic su uno spazio vuoto propone un
  evento di 30 minuti a quell'orario. Gli eventi sovrapposti sono
  affiancati, quelli dell'intera giornata e le attività in scadenza stanno
  in alto. Spostando l'inizio di un evento la durata resta quella scelta
  anche se è inferiore a un'ora.
- **Tabelle: `Ctrl+A` seleziona tutta la tabella** quando il cursore è in
  una cella, così allineamento e formattazione si applicano a tutte le
  celle.

- **Ricerca nel menu «Altro» dell'editor.** Un campo in cima al menu trova
  i comandi di formattazione per nome, compresi quelli delle tabelle.
- **Collegamenti interni nel server MCP.** `read_note` riporta i
  collegamenti della nota verso altre note, con gli id per leggerle;
  quelli verso taccuini esclusi dall'AI non compaiono.
- **Modifiche AI: vista «Originale».** L'anteprima di una modifica AI
  mostra il testo di partenza, il risultato o il confronto parola per
  parola.

- **Nuovi comandi nell'editor.** Nel menu **Altro**: allineamento a
  sinistra, al centro e a destra, aumenta e riduci rientro, apice e
  pedice. Il link ha un pulsante proprio nella barra. Allineamento e
  rientro si conservano nell'esportazione HTML e PDF, apice e pedice anche
  in Markdown; l'importazione HTML riconosce allineamento, apice e pedice.
  I menu in fondo alla barra non escono più dal bordo della finestra.

- **Titolo aggiornato nella lista delle note.** Dopo aver cambiato il
  titolo di una nota (per esempio appena creata), la colonna centrale
  continuava a mostrare quello vecchio, come «Nota senza titolo», finché
  la lista non veniva ricaricata.

## 0.2.4 — 2026-11-11

- **Aggiornamento dall'app su Fedora 43 e successive.** Con il pacchetto
  `.rpm` l'aggiornamento chiedeva la password due volte e poi falliva:
  RPM 6 rifiuta i pacchetti senza firma OpenPGP. Ora Greatnote installa
  il `.rpm` già verificato con la propria firma chiedendo la password una
  sola volta, e in caso di errore mostra il motivo indicato da `rpm`. Chi
  ha la 0.2.3 o precedente deve installare questa versione a mano una
  volta: `sudo rpm -U --nosignature Greatnote.x86_64.rpm`.

- **Dipendenze aggiornate per sicurezza.** `rusqlite` 0.42 (SQLite inclusa
  senza la vulnerabilità RUSTSEC-2026-0459) e `pulldown-cmark` 0.13.6.
  L'avviso su `crypto-bigint`, senza correzione disponibile e su codice
  che Greatnote non usa, è registrato come eccezione in
  `.cargo/audit.toml` (vedi `docs/SECURITY.md`).

## 0.2.3 — 2026-11-10

- **Ricerca in un riquadro.** Il pulsante di ricerca della barra laterale
  (e `Ctrl+K`/`Ctrl+F`) apre un riquadro sopra la finestra invece di
  spostare il cursore nella colonna delle note: mostra le note recenti,
  cerca in titolo e testo evidenziando i termini trovati, permette di
  limitare la ricerca al taccuino, spazio o tag aperto e di passare alle
  sezioni dell'app. Sostituisce la finestra «Vai a…».

## 0.2.2 — 2026-10-23

- **Aggiornamenti più affidabili su Windows.** Un'installazione fatta con
  l'installer `.msi` riceveva l'aggiornamento come installer `.exe`, che
  installava una seconda copia in un'altra cartella: riaprendo l'app
  partiva ancora la versione vecchia. Ora ogni installazione riceve lo
  stesso tipo di installer.
- **Aggiornamento scaricato conservato.** Il pacchetto scaricato resta sul
  disco (con la firma ricontrollata a ogni avvio) finché non viene
  installato: chiudere l'app o un'installazione non riuscita non
  obbligano più a scaricarlo di nuovo.
- **Pulsante «Riavvia e aggiorna» leggibile.** Passandoci sopra con il mouse
  il testo diventava dello stesso colore dello sfondo.

## 0.2.1 — 2026-10-22

Sostituisce la 0.2.0, che non è stata pubblicata: i test su Windows
fallivano. Contiene tutte le novità della 0.2.0 (vedi sotto).

- **Testo cifrato uguale su Windows e Linux.** Su Windows il testo cifrato
  veniva scritto con a capo diversi da Linux: lo stesso blocco risultava
  diverso tra i computer sincronizzati e non coincideva dopo
  un'esportazione in Markdown. Ora il formato è uno solo; i blocchi con a
  capo di Windows si aprono comunque.

## 0.2.0 — 2026-10-22 (non pubblicata)

Il database passa alla versione 15: all'avvio viene creato il backup
`greatnote-premigration-…` e, dopo l'aggiornamento, le versioni 0.1.x non
lo aprono più (vedi «Ritorno alla versione precedente» nella checklist di
rilascio). Con la sincronizzazione attiva aggiorna tutti i computer.

- **Modifiche dall'app di AI tramite MCP (facoltative).** In Impostazioni ›
  Integrazioni, **Consenti anche le modifiche** permette a Claude Code e
  alle altre app MCP di creare note, aggiungere testo in fondo a una nota,
  creare attività e completarle. Non possono eliminare né riscrivere nulla;
  il permesso è spento per impostazione predefinita e si revoca subito. Il
  nuovo strumento `list_tasks` elenca le attività anche in sola lettura.
- **Attività che si ripetono.** Nella modifica di un'attività, **Ripeti**
  la ripete ogni giorno, settimana, mese o anno, anche ogni N (per esempio
  ogni 2 settimane), in giorni della settimana scelti e fino a una data.
  Completandola compare la successiva, con i promemoria spostati alla nuova
  data; le scadenze saltate non vengono ricreate. Il database passa alla
  versione 15 (backup automatico prima dell'aggiornamento).
- **Aggiornamenti automatici anche su Linux.** Da questa versione la
  release contiene gli aggiornamenti firmati per AppImage, `.deb` e `.rpm`:
  Greatnote installato su Linux trova, scarica e verifica da solo le nuove
  versioni (anche la 0.1.2 trova questa). L'AppImage si aggiorna alla chiusura;
  con `.deb` e `.rpm` l'installazione parte con **Riavvia e aggiorna** e
  chiede la password di amministratore.
- **Pacchetti Linux nei centri software.** Aprendo il `.rpm` o il `.deb`,
  Discover mostrava «Greatnote-0» (il nome del file fino al primo punto).
  Nelle release i pacchetti si chiamano ora `Greatnote.x86_64.rpm`,
  `Greatnote.amd64.deb` e `Greatnote.x86_64.AppImage`. Dopo
  l'installazione Discover e GNOME Software mostrano Greatnote con icona,
  descrizione e versione (metadati AppStream), e il menu delle applicazioni
  ha l'icona anche nelle dimensioni piccole.
- **Tasto destro in tutta l'app.** Il menu a comparsa offre le azioni utili
  dell'elemento cliccato, come nelle app desktop: **Svuota cestino** sul
  Cestino, **Nuova nota in questo taccuino** sui taccuini, rinomina ed
  elimina su spazi, taccuini e tag, **Apri in una nuova scheda** sulle voci
  della barra laterale, taglia/copia/incolla e formato nel testo della
  nota, **Apri collegamento** sui link, chiudi le altre schede, e azioni
  rapide su attività, promemoria, eventi, file, modelli, allegati ed
  elementi del cestino. Nei campi di testo compare il menu standard con
  taglia, copia e incolla. Il menu del browser integrato («Ricarica»,
  «Indietro») non compare più.
- **Testo cifrato nelle note.** Una parte di una nota (un PIN, dei dati di
  accesso) si può proteggere con una password: dal menu `/`, da
  **＋ Inserisci** o selezionando il testo e premendo `Ctrl+Shift+X`. Nella
  nota resta un riquadro con il lucchetto; con un clic e la password il
  testo si legge, si copia, si modifica o torna normale. La password non
  viene salvata e il testo cifrato non compare nella ricerca né viene
  inviato all'AI. Il formato è quello aperto di age, decifrabile anche con
  `age -d`.
- **Testo dalle immagini con Gemma.** Con il motore integrato, leggendo il
  testo di un'immagine Gemma 4 scriveva anche il proprio ragionamento in
  inglese («The user wants me to transcribe…») e a volte non la
  trascrizione. Ora il risultato contiene solo il testo dell'immagine;
  la lettura richiede qualche secondo in più.

Limiti noti:

- gli installer Windows non sono firmati con un certificato di firma del
  codice: SmartScreen mostra un avviso;
- le installazioni Linux 0.1.2 da `.deb` o `.rpm` installano questa
  versione alla chiusura chiedendo la password (comportamento della 0.1.2);
  dalla 0.2.0 l'installazione dei pacchetti di sistema avviene solo con
  **Riavvia e aggiorna**;
- gli eventi del calendario e i promemoria sulle note non si ripetono
  ancora; gli strumenti MCP non possono eliminare né riscrivere.

## 0.1.2 — 2026-10-22

Prima versione con pacchetti Linux (.deb, .rpm, AppImage) allegati alla
release, insieme agli installer Windows.

- **Linux con il motore AI integrato.** I pacchetti Linux (.deb, .rpm,
  AppImage) includono lo stesso motore llama.cpp della versione Windows:
  - nessun Ollama o altro server da installare;
  - modelli scaricabili dalle impostazioni;
  - usa la scheda video tramite Vulkan (NVIDIA, AMD, Intel).
- **Un solo progetto per Linux e Windows.** Il repository ora si chiama
  `tihcio/greatnote`. Le versioni già installate continuano a ricevere gli
  aggiornamenti.
- **Sincronizzazione più reattiva.** Durante l'invio e la ricezione in rete
  (cartella o Dropbox) il database non resta più bloccato, quindi l'app
  risponde anche mentre sincronizza molti allegati.
- **Voce di menu su Linux.** Greatnote compare tra le applicazioni da
  ufficio, con una descrizione in italiano, invece che in «Altro».

Limiti noti:

- su Linux l'app non si aggiorna da sola: scarica il nuovo pacchetto dalla
  pagina della release e installalo sopra quello vecchio (i dati restano).
  In Impostazioni › Aggiornamenti il controllo risponde «Non ci sono
  aggiornamenti automatici per questo tipo di installazione»;
- gli installer Windows non sono firmati con un certificato di firma del
  codice: SmartScreen mostra un avviso.

## 0.1.1 — 2026-10-22

- **Modelli dal Cestino.** Usando un modello dopo aver aperto il Cestino, la
  nota creata si apriva in sola lettura come se fosse nel cestino. Ora si
  apre in Tutte le note ed è modificabile. Lo stesso vale tornando a una
  nota con le schede o con Indietro/Avanti.

- **Calendario.** Nella vista mese gli eventi con titolo lungo restano nel
  riquadro del giorno (il titolo termina con «…») invece di invadere i giorni
  accanto. Un giorno con più eventi allunga la propria settimana invece di
  sovrapporsi a quella successiva.

## 0.1.0 — 2026-10-21

Prima versione pubblicata, per Windows x64 (installer NSIS e MSI non firmati
con un certificato di firma del codice). Da questa versione gli
aggiornamenti successivi arrivano direttamente nell'app.

- **Aggiornamenti automatici.** Greatnote per Windows cerca da solo le
  nuove versioni, le scarica in background e le installa quando lo chiudi
  (o subito con **Riavvia e aggiorna**). In basso nella barra laterale
  compare **Aggiornamento X pronto**; in Impostazioni › Aggiornamenti si
  vedono versione e novità e si possono disattivare. Gli aggiornamenti sono
  verificati con la firma del progetto (ADR 0023).

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
