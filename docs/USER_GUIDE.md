# Guida utente

## Stato iniziale

Il progetto include la shell desktop e la gestione locale delle note,
dell'organizzazione, delle bozze recuperabili, del cestino, della ricerca,
dell'import/export e dei backup, ma non è ancora un rilascio stabile.
L'app conserva i dati nella directory dati utente; non è necessario un
account o un servizio online.

L'interfaccia è realizzata con componenti web standard e non richiede KDE.
Tauri usa il window manager e il runtime WebKitGTK disponibili sulla
distribuzione Linux.

## Avvio per sviluppatori

Consultare [README.md](../README.md) per installazione, avvio, test e build.

## Interfaccia e menu

La finestra ha una **barra dei menu** (File, Modifica, Visualizza, Nota,
Finestra, Strumenti, Aiuto) e, sotto, una **barra delle schede** con
← e → per tornare alle posizioni precedenti. Ogni voce di menu esegue
un'azione; quelle non disponibili in quel momento (es. "Esporta nota" senza
una nota aperta) sono disattivate. **Aiuto › Scorciatoie da tastiera**
(`Ctrl+/`) elenca tutte le combinazioni.

- **File:** nuova nota (`Ctrl+N`), nuovo taccuino (`Alt+Shift+N`), spazio
  (`Alt+Shift+S`), evento (`Ctrl+E`), attività (`Alt+T`), tag
  (`Ctrl+Shift+T`), nuova nota da modello, salva ora (`Ctrl+S`), importa,
  esporta (Markdown, HTML, testo, PDF), stampa (`Ctrl+P`), dati e backup,
  esci.
- **Modifica:** annulla/ripeti, taglia, copia, incolla, incolla senza
  formattazione (`Ctrl+Shift+V`), seleziona tutto, cerca nelle note
  (`Ctrl+K` o `Ctrl+F`), **Vai a…** (`Ctrl+Q`: note, taccuini, tag e
  sezioni), trova e sostituisci (`Ctrl+H`).
- **Visualizza:** ricarica i dati (`Ctrl+R`) o la finestra
  (`Ctrl+Shift+R`), indietro/avanti (`Alt+←`/`Alt+→`), zoom (`Ctrl+0`,
  `Ctrl++`, `Ctrl+-`), schermo intero (`F11`), barra laterale (`F10`),
  opzioni della barra laterale e tema (sistema, chiaro, scuro).
- **Nota:** apri in una nuova finestra (`Alt+O`) o scheda (`Alt+Shift+O`),
  vista della nota (normale, editor ampliato, concentrazione), condividi
  come testo (copia come Markdown o testo), sposta in (`Alt+Shift+M`), copia
  in, duplica, modifica tag (`Alt+Ctrl+T`), scorciatoie, archivia, salva come
  modello, copia link, trova e sostituisci, larghezza, comandi rapidi,
  informazioni (`Ctrl+Shift+I`), cronologia, sposta nel cestino (`Canc`
  nell'elenco).
- **Finestra:** nuova scheda (`Alt+Shift+T`), successiva/precedente
  (`Ctrl+Tab`, `Ctrl+Shift+Tab`), seleziona scheda (`Alt+1`…`Alt+9`), chiudi
  scheda (`Ctrl+W`), riduci a icona (`Ctrl+M`).
- **Strumenti:** impostazioni (`Ctrl+,`), dati e backup, controllo
  d'integrità.
- **Menu contestuale:** tasto destro (o `Shift+F10`) su una nota
  nell'elenco, oppure il pulsante **⋯** sopra la nota.

**Barra laterale:** Home (note recenti, scorciatoie, attività di oggi,
prossimi eventi, promemoria da gestire), Scorciatoie, Tutte le note,
Attività, Promemoria, File (tutti gli allegati), Calendario, Modelli,
Altro (Recenti, Archivio), Cestino, poi note recenti, spazi, taccuini e tag.
Da **Visualizza** o **Impostazioni** puoi mostrare il numero di note,
nascondere le note recenti o i pulsanti attività/evento, raggruppare in
"Altro" e scegliere se elencare tra i taccuini anche quelli negli spazi.

**Editor:** **Inserisci** (elenchi, checklist, citazione, codice, tabella,
separatore, data, link, allegato, attività collegata), annulla/ripeti,
stile (testo o titoli), carattere, dimensione, colore, grassetto, corsivo,
sottolineato, evidenziazione, elenchi e **Altro** (barrato, codice, link,
rimuovi formattazione, operazioni sulle tabelle). Digita **/** a inizio riga
per i comandi rapidi. Allegati, collegamenti, attività e promemoria della
nota sono sotto il testo.

**Note e modelli:** **Duplica** crea una copia con tag e allegati; **Copia
in…** la crea in un altro taccuino. **Salva come modello…** salva la nota tra
i Modelli, da cui **Usa** crea una nuova nota (ci sono anche tre modelli
predefiniti). **Cronologia nota** mostra le versioni salvate
automaticamente (al massimo ogni 10 minuti di modifiche, ultime 50) e
permette di ripristinarle; la versione sostituita resta nella cronologia.
Per ogni versione la scheda **Differenze con l'attuale** evidenzia in verde
il testo aggiunto da allora e in rosso quello rimosso. Se l'assistente AI è
attivo, **Spiegazione AI** riassume le modifiche: la versione scelta e
quella attuale vengono inviate al modello solo quando premi **Spiega le
differenze con l'AI**.
**Copia link della nota** copia `greatnote://note/…`: nel pannello
Collegamenti **Incolla link** crea il collegamento a quella nota.

**Finestra separata:** "Apri in una nuova finestra" mostra la nota da sola,
con salvataggio automatico. Nella finestra principale la nota viene chiusa
per non modificarla in due punti contemporaneamente.

## Larghezza delle colonne

Passa il mouse sul bordo tra la barra laterale e l'elenco delle note, o tra
l'elenco e l'editor: compare una linea verde e il puntatore cambia. Trascina
per allargare o stringere la colonna; la larghezza viene ricordata. Con un
doppio clic sul bordo torna quella predefinita. Da tastiera: Tab fino al
bordo, poi frecce sinistra/destra (con Maiusc a passi più grandi).

## Note e organizzazione

- **Nuova nota:** usa il pulsante nella barra laterale o `Ctrl/Cmd+N`.
  Se hai selezionato un taccuino, la nuova nota viene assegnata direttamente
  a quel taccuino e la vista resta sul taccuino.
- **Spazi:** crea uno spazio dal pulsante `+` nella sezione Spazi. Il menu
  `⋯` consente di rinominarlo; `×` lo sposta nel cestino dopo conferma. I
  taccuini dello spazio restano disponibili senza spazio e tornano nello
  spazio se lo ripristini.
- **Taccuini:** crea un taccuino dal pulsante `+` nella sezione Taccuini o
  accanto a uno spazio per associarlo subito. Dal menu `⋯` puoi rinominarlo o
  assegnarlo a uno spazio; `×` lo sposta nel cestino: le note restano in
  **Senza taccuino** e tornano nel taccuino se lo ripristini.
- **Tag:** crea tag dalla relativa sezione, rinominali o spostali nel
  cestino. I tag della nota sono nella barra in fondo all'editor: scrivi in
  **Aggiungi tag** per scegliere fra i suggerimenti o premi Invio per crearne
  uno nuovo; `×` (o Backspace sul campo vuoto) lo toglie. Un tag nel cestino viene
  tolto dalle note e riassegnato se lo ripristini; finché è nel cestino il suo
  nome non si può riusare.
- Seleziona scorciatoie, recenti, archivio, cestino, spazio, taccuino o tag
  nella barra laterale per filtrare l'elenco; usa **Senza taccuino** per
  trovare le note non organizzate.
- Il taccuino della nota è nel percorso in alto (es. «Progetti › Titolo»):
  cliccalo per spostare la nota. Il pulsante con le due frecce a sinistra
  amplia l'editor, quello accanto attiva la concentrazione; ⋯ contiene
  archivia, cestino e le altre azioni. L'elenco delle note si mostra a schede
  o a righe con il pulsante sopra la ricerca.
  Dal Cestino seleziona una nota e scegli **Ripristina nota**, oppure
  **Elimina definitivamente** per cancellarla con allegati, collegamenti e
  promemoria (le attività collegate restano, senza nota). **Svuota cestino**,
  accanto al conteggio delle note, elimina definitivamente tutto il contenuto
  del cestino.
- Anche spazi, taccuini, tag, attività ed eventi eliminati finiscono nel
  cestino: li trovi nella sezione **Altri elementi** in cima alla vista
  Cestino, con **Ripristina** e **×** (elimina definitivamente). Attività ed
  eventi nel cestino non compaiono nelle viste e i loro promemoria non
  scattano. Entrambe le operazioni chiedono conferma in un dialog (il
  pulsante attivo all'apertura è **Annulla**; Esc annulla) e non si possono
  annullare; le note restano soltanto nei backup creati in precedenza.
- Le modifiche al titolo e al contenuto vengono salvate localmente in
  automatico. In caso di errore il messaggio resta visibile e puoi riprovare.
- Se una chiusura anomala lascia una bozza non applicata, all'apertura della
  nota l'app chiede se ripristinarla. Puoi scartarla per mantenere la versione
  salvata precedente; la bozza non la sovrascrive senza conferma.

## Ricerca, allegati e collegamenti

- La ricerca è locale. Puoi combinare testo/frasi tra virgolette con filtri
  come `tag:progetto`, `notebook:personale`, `space:lavoro`,
  `created:2026-01-01..2026-12-31`, `updated:7d`, `has:attachment`,
  `is:pinned`, `is:archived`, `is:task` (note con attività aperte),
  `before:2026-10-01` e `after:2026-01-01`.
  Gli operatori booleani e le parentesi non sono supportati e mostrano un
  errore esplicito.
- Le parole valgono anche come inizio di parola (`euron` trova
  «Euronics»), così i risultati compaiono mentre scrivi; per una frase
  esatta usa le virgolette. La ricerca comprende titolo, testo, allegati di
  testo e il testo letto dalle immagini; l'anteprima mostra il punto trovato,
  anche quando è nell'immagine.
- Usa la stella nella barra dell'editor per aggiungere/rimuovere una preferita.
  **Archivia nota** la sposta nell'Archivio senza cestinarla; dall'Archivio
  puoi ripristinarla. Le note recenti sono quelle aperte più di recente.
- **Aggiungi allegato** apre il selettore di file; puoi anche trascinare un
  file nell'area della nota. Sono accettati testi, PNG, JPEG, GIF e PDF fino
  a 25 MiB. L'app verifica il contenuto, lo salva nella directory locale
  protetta e verifica SHA-256 prima di aprirlo con l'app predefinita.
- Nel pannello **Collegamenti tra note**, seleziona una nota per creare un
  collegamento. Usa il titolo collegato per aprire la destinazione; puoi
  rimuovere il collegamento senza eliminare le note.
- Scorciatoie: `Ctrl/Cmd+N` nuova nota, `Ctrl/Cmd+F` ricerca,
  `Ctrl/Cmd+S` flush locale, `Ctrl/Cmd+Shift+N` nuovo taccuino. Undo/redo
  (`Ctrl/Cmd+Z` e `Ctrl/Cmd+Shift+Z`) sono gestiti dall'editor.

## Attività, promemoria e calendario

- **Attività** (barra laterale): scrivi il titolo nel campo "Nuova
  attività…", scegli scadenza e priorità e premi Invio. Le schede Oggi, In
  ritardo, Prossime, Senza scadenza, Tutte aperte e Completate dividono le
  attività in base alla data di oggi; il numero accanto ad "Attività" somma
  quelle di oggi e in ritardo. Il cerchio a sinistra completa o riapre
  un'attività; il titolo apre la modifica (descrizione, scadenza, priorità,
  promemoria, eliminazione definitiva con conferma).
- **Attività di una nota:** nel pannello "Attività" sotto la nota puoi
  aggiungere attività collegate. Dall'elenco delle attività il nome della nota
  la apre. Se la nota va nel cestino, l'attività resta.
- **Promemoria:** puoi aggiungerli a una nota (pannello "Promemoria"), a
  un'attività o a un evento scegliendo la data dal calendario e ore e minuti
  dai due elenchi (passi di 5 minuti). All'orario previsto
  Greatnote mostra una notifica di sistema e un riquadro in basso a destra che
  resta finché non scegli un'azione: **Apri**, **+10 min**, **+1 ora**,
  **Domani** (stessa ora locale), **Fatto** (per un'attività la completa) o
  elimina. La sezione **Promemoria** elenca quelli da gestire, quelli
  programmati e, in **Gestiti**, lo storico di quelli già gestiti (anche
  quelli chiusi completando un'attività), con l'orario previsto e il momento
  in cui sono stati gestiti. Dallo storico puoi aprire l'elemento o eliminare
  i promemoria, singolarmente o con **Svuota storico**.
- **App chiusa:** i promemoria scattano solo mentre Greatnote è aperta. Se
  l'app era chiusa o il computer sospeso, al successivo avvio ricevi la
  notifica "Scaduto alle …: perso mentre l'app era chiusa" e il promemoria
  resta tra quelli da gestire. Se all'avvio ci sono promemoria non ancora
  gestiti di sessioni precedenti, una notifica li riepiloga. Le notifiche
  restano visibili finché non le chiudi. Non c'è un servizio in
  background.
- **Calendario:** vista mensile o settimanale, con i pulsanti ‹ › e Oggi.
  Il `+` in un giorno (o "Nuovo evento") crea un evento con orario (data,
  ore e minuti di inizio e fine; spostando l'inizio la fine mantiene la
  durata) o per l'intera giornata; clic su un evento per modificarlo, aggiungere promemoria
  o eliminarlo. Le scadenze delle attività compaiono nel giorno di scadenza
  con una casella per completarle. Gli orari sono mostrati nel fuso del
  sistema, indicato sotto il titolo. Gli eventi dei calendari Google e
  Microsoft collegati hanno il colore del loro calendario (vedi
  [Calendari esterni](#calendari-esterni-google-e-microsoft)).
- **Fusi orari e ora legale:** le scadenze sono date (non orari) e non
  cambiano con il fuso. Promemoria ed eventi sono istanti precisi: se cambi
  fuso vengono mostrati all'ora locale corrispondente. Un orario che non
  esiste per il passaggio all'ora legale (es. 02:30 dell'ultima domenica di
  marzo in Italia) viene spostato alle 03:30; un orario ripetuto al ritorno
  all'ora solare usa la prima occorrenza.

## Esportare una nota

Nella barra dell'editor, il menu **Esporta** salva la nota corrente come
Markdown (con titolo, date, taccuino e tag nel front matter), HTML autonomo o
testo semplice. **PDF / Stampa…** apre il dialog di stampa del sistema: scegli
"Stampa su file"/"Salva come PDF". Prima dell'esportazione le modifiche in
corso vengono salvate.

## Dati e backup

Apri **Dati e backup** dal fondo della barra laterale.

- **Importa**: scegli file Markdown/HTML/testo, una cartella di Markdown
  (sottocartelle incluse, file nascosti e collegamenti simbolici esclusi) o un
  archivio Greatnote. Viene mostrata un'anteprima con note nuove, duplicate e con
  errori: nulla cambia finché non confermi. Per i file la destinazione
  predefinita è un nuovo taccuino "Importazione AAAA-MM-GG"; l'archivio Greatnote
  ripristina spazi, taccuini, tag, collegamenti e allegati. L'importazione è
  tutto-o-niente: se si verifica un errore imprevisto non viene importato
  nulla. Gli allegati elencati nel front matter Markdown vengono importati se
  si trovano dentro la cartella scelta. HTML importato: script, stili,
  attributi e link non sicuri vengono rimossi.
- **Esporta**: ZIP Markdown di un taccuino o di tutte le note (con allegati),
  oppure archivio completo reimportabile. Il cestino non è esportato.
- **Backup**: crea un backup, verificalo, salvane una copia altrove (per
  esempio su un disco esterno) o ripristinalo. Il backup automatico all'avvio
  è attivo per impostazione predefinita (ogni 7 giorni, ultimi 10
  conservati); i backup manuali non vengono mai eliminati automaticamente.
- **Copia cifrata**: accanto a ogni backup, **Copia cifrata…** chiede una
  password (almeno 10 caratteri, da ripetere) e salva un file `.zip.age`.
  La password non viene salvata da nessuna parte: **se la dimentichi la copia
  non è recuperabile**. Il file usa il formato aperto age e si può decifrare
  anche senza Greatnote con `age -d copia.zip.age > copia.zip`.
- **Integrità**: controlla database, indice di ricerca e allegati; puoi
  ricostruire l'indice di ricerca se mancano risultati.

## Procedura di recupero dati

1. Apri **Dati e backup → Integrità** e avvia la verifica. Se segnala note non
   indicizzate, usa **Ricostruisci indice di ricerca**.
2. Se il database è danneggiato o mancano allegati, vai su **Backup**, usa
   **Verifica** su un backup recente e poi **Ripristina**. Prima del
   ripristino l'app crea un backup "Prima del ripristino" dello stato
   corrente: puoi tornare indietro ripristinando quello.
3. Per recuperare dati su un altro computer, copia il file di backup con
   **Salva copia…** o **Copia cifrata…**, installa Greatnote e usa
   **Ripristina da file…**. Per un file `.age` viene chiesta la password:
   il backup viene decifrato in una copia temporanea privata, verificato e
   solo dopo la tua conferma ripristinato; la copia in chiaro viene poi
   eliminata.
4. Se l'app non si avvia dopo un aggiornamento, nella cartella `backups/` è
   presente un backup `greatnote-premigration-….zip` (o `gna-premigration-…`
   per le versioni precedenti al cambio di nome) creato prima dell'aggiornamento
   dello schema; può essere ripristinato con **Ripristina da file…** da
   un'installazione funzionante della stessa versione o successiva.

I backup nella cartella dell'app contengono note e allegati **non cifrati**,
protetti solo dai permessi del sistema: per conservarli altrove (disco
esterno, cloud) usa **Copia cifrata…**.

## Importare da Evernote, Joplin e Obsidian

In **File › Importa…** (scheda Importa), sezione **Da altre app**:

- **Evernote (.enex)**: in Evernote esporta ogni taccuino come file ENEX
  (clic destro sul taccuino › Esporta taccuino). Puoi scegliere più file
  insieme: ognuno diventa un taccuino con il suo nome.
- **Joplin (.jex)**: in Joplin usa **File › Esporta tutto › JEX** (o
  l'esportazione di un taccuino in JEX). Vanno bene anche i file
  `all_notebooks.jex` del plugin di backup.
- **Vault di Obsidian**: scegli la cartella del vault.

Prima di importare vedi quante note sono nuove, duplicate o con errori, il
numero di taccuini, tag, allegati e collegamenti, e gli avvisi per singola
nota. La destinazione proposta è **Mantieni i taccuini d'origine**, raccolti
in un nuovo spazio (puoi cambiarne il nome o toglierlo); le cartelle
annidate diventano taccuini come «Lavoro › Progetti». In alternativa puoi
mettere tutto in un solo taccuino.

Vengono mantenuti titoli, testo e formattazione, tag (anche i `#tag` e il
front matter di Obsidian), date di creazione e modifica, allegati e
collegamenti tra note (pannello Collegamenti). Immagini e PDF incorporati
diventano allegati della nota e nel testo resta «📎 nome» dove si trovavano.
Non vengono importati: allegati di tipo non supportato (audio, video,
documenti Office: indicati negli avvisi), testo cifrato di Evernote, canvas
e impostazioni di Obsidian. Le attività di Joplin diventano note.
Reimportare la stessa raccolta salta le note già presenti.

## Immagini nelle note

Le immagini che alleghi (pulsante **Aggiungi allegato**, trascinamento nella
nota o incolla con `Ctrl+V`) compaiono nel testo dove si trova il cursore.
Per le immagini già allegate usa **Inserisci nel testo** nel pannello
Allegati. Le immagini incorporate nelle note importate da Evernote, Joplin
e Obsidian compaiono al loro posto.

**Dimensione:** passa il mouse sull'immagine (o selezionala) e trascina la
maniglia nell'angolo in basso a destra, oppure usa **Piccola**, **Media**,
**Grande** e **Originale** nella barra in alto a destra dell'immagine.

**Annotazioni:** **✎ Annota** (o doppio clic sull'immagine) apre l'editor:
penna, evidenziatore, freccia, rettangolo, cerchio, **T** per scrivere una
nota (clic dove scriverla, Invio per confermare) e gomma (clic su un segno).
Scegli colore e spessore, annulla con `Ctrl+Z`, salva con **Salva
annotazioni** o `Ctrl+Invio`. L'immagine originale non viene modificata: le
annotazioni stanno nella nota, si annullano anche dall'editor con `Ctrl+Z` e
le note scritte sulle immagini diventano ricercabili. Un'immagine il cui allegato è stato
rimosso viene indicata come non disponibile.

**Testo nelle immagini:** **Leggi il testo** (pannello Allegati) estrae il
testo di uno scontrino, di una lavagna o di un documento fotografato; il
risultato si apre con **Testo letto**, diventa ricercabile e l'assistente AI
lo usa per rispondere. In Impostazioni › Intelligenza artificiale › Testo
nelle immagini scegli il motore: il modello AI con visione (per esempio
Claude Code, che riceve l'immagine) oppure Tesseract installato sul computer
(`sudo dnf install tesseract tesseract-langpack-ita`, nessun invio). Puoi
leggere automaticamente le immagini appena allegate o tutte quelle già
presenti.

## Assistente AI (facoltativo)

Le funzioni AI sono **spente** finché non le attivi in **Strumenti ›
Impostazioni › Intelligenza artificiale**.

**Motore integrato (il modo più semplice).** Non serve installare nulla:

1. Attiva le funzioni AI e scegli il provider **Integrato**.
2. Premi **Scarica i consigliati** oppure scarica i singoli modelli, per
   ognuna delle tre sezioni:
   - **Chat e modifiche**: obbligatoria.
   - **Contesto**: facoltativa. Serve alla ricerca per significato e a
     dare alla chat le note pertinenti.
   - **Visione**: facoltativa. Serve a leggere il testo nelle immagini.

   Per ogni modello sono indicati dimensione, memoria necessaria e
   licenza. I file arrivano da Hugging Face, sono verificati con il loro
   hash e restano sul computer. Puoi interrompere un download e
   riprenderlo dopo.
3. Scegli il modello di ogni sezione (i modelli scaricati vengono
   proposti da soli) e premi **Salva le impostazioni AI**.

**Scegliere il modello adatto al tuo computer.** In cima alla sezione
compaiono la RAM e le schede video del computer. Ogni modello ha
un'etichetta:

- **Adatto: scheda video**: risposte rapide.
- **Adatto: processore, più lento**: funziona, ma le risposte sono più
  lente.
- **Al limite della memoria**: può rallentare il computer.
- **Troppo grande**: probabilmente non si avvia.

**Altri modelli da Hugging Face.** Sotto ogni sezione c'è **Cerca altri
modelli su Hugging Face**:

1. Apri la ricerca: compaiono i modelli GGUF più scaricati per quella
   sezione. Puoi anche cercarne uno per nome (per esempio gemma, qwen,
   mistral).
2. Premi **Mostra le varianti** su un modello. Ogni variante ha
   dimensione, memoria stimata ed etichetta. Quella **Consigliata per
   questo computer** è la più fedele che ci sta.
3. Premi **Scarica**: la conferma riporta dimensione, licenza e
   adattezza. Il modello compare nell'elenco della sezione con il marchio
   «Hugging Face» e si sceglie come gli altri.

La ricerca invia a Hugging Face solo le parole cercate, mai le note. I
modelli trovati non sono stati provati con il motore incluso: se uno non
si avvia, l'errore lo dice e puoi sceglierne un altro. Controlla anche la
licenza, perché alcune non permettono ogni uso. I modelli che richiedono
di accettare condizioni su Hugging Face non sono scaricabili dall'app. Un
download interrotto si riprende con **Scarica** o si toglie dall'elenco
con **Elimina**.

Il primo utilizzo di un modello richiede qualche secondo per caricarlo.
Dopo 10 minuti di inattività il modello viene chiuso per liberare memoria.
Con una scheda grafica compatibile Vulkan le risposte sono più rapide;
senza, Greatnote usa solo il processore. Un modello si elimina con
**Elimina**; quello in uso va prima sostituito.

Su Windows il motore è incluso nell'installer. Su Linux serve llama.cpp
installato (comando `llama-server` nel `PATH`). In alternativa puoi
indicare il percorso completo dell'eseguibile nella variabile d'ambiente
`GREATNOTE_LLAMA_SERVER`.

**Server esterno.** In alternativa servono un server di modelli
compatibile con l'API OpenAI e almeno un modello di chat già installato:

1. Installa e avvia un server, per esempio Ollama, e scarica un modello
   (`ollama pull qwen3:8b`). Per la ricerca per significato serve anche un
   modello di embedding (`ollama pull nomic-embed-text`).
2. Nelle impostazioni attiva le funzioni AI, scegli il preset (Ollama, LM
   Studio, llama.cpp) o scrivi l'indirizzo, premi **Prova la connessione**,
   indica i modelli e premi **Salva le impostazioni AI**.
3. Facoltativo: scegli i **taccuini esclusi** (mai inviati né indicizzati) e
   il testo massimo inviato per richiesta.

**Open WebUI** (porta 3000): usa il preset Open WebUI
(`http://localhost:3000/api`, non l'indirizzo della pagina web). Open WebUI
richiede sempre una chiave: creala in Open WebUI › Impostazioni › Account ›
Chiavi API e incollala nel campo **Chiave API** (la prova di connessione la
usa anche prima del salvataggio; **Salva le impostazioni AI** la mette nel
portachiavi). I nomi dei modelli sono quelli mostrati da Open WebUI.

**Claude Code**: se hai Claude Code installato e hai effettuato l'accesso
(nel terminale: `claude`), scegli il provider **Claude Code**, lascia vuoto il
percorso (viene trovato da solo) o indicalo, scegli il modello (o quello
predefinito dell'account), spunta il consenso all'invio ad Anthropic e premi
**Verifica Claude Code**. Claude Code viene usato solo come modello di
testo: non può leggere file né eseguire comandi. Non offre embedding, quindi
la ricerca per significato non è disponibile con questo provider.

Con un server remoto (o Claude Code) la prima richiesta mostra cosa verrà
inviato: lasciando spuntato **Non chiedermelo più** le richieste successive
partono subito. Per tornare a confermare ogni invio attiva **Mostra cosa
verrà inviato e chiedi conferma prima di ogni richiesta** nelle impostazioni
AI.

Un server che non è sul tuo computer deve usare `https` e richiede la
casella di consenso. La chiave API, se serve, viene salvata nel portachiavi
di sistema (KWallet o GNOME Keyring) e puoi eliminarla in ogni momento.

**Assistente AI** (barra laterale o `Ctrl+Shift+A`):

- **Chat**: fai una domanda; l'assistente cerca le note pertinenti e
  risponde citandole come `[1]`, `[2]`. Clicca un numero o l'elenco
  "fonti usate" per aprire la nota. Con un server remoto vedi prima i brani
  che verranno inviati e puoi togliere quelli che non vuoi. La conversazione
  non viene salvata.
- **Cerca per significato**: trova note con parole diverse da quelle
  cercate; per ogni risultato vedi il brano più simile. Richiede l'indice
  semantico: **Aggiorna l'indice** (o l'aggiornamento automatico nelle
  impostazioni). Le note modificate escono dai risultati finché non sono
  reindicizzate.

**Sulla nota aperta** (pulsante **✦ AI** nella barra dell'editor o
**Strumenti › AI sulla nota**): chiedi all'assistente su questa nota,
correggi grammatica, riscrivi con un tono, riassumi, espandi, traduci, crea
un elenco di punti, estrai decisioni e azioni, migliora la formattazione.
L'azione usa il testo selezionato o, senza selezione, l'intera nota. Il
risultato compare in anteprima con **Confronta** (parti aggiunte e tolte) e
viene applicato solo con **Sostituisci** o **Inserisci sotto**; puoi
annullarlo con `Ctrl+Z`. **Suggerisci tag** ed **Estrai attività** propongono
elementi che puoi correggere e scegliere prima di aggiungerli.

**Cronologia**: la terza scheda dell'Assistente elenca le richieste fatte
all'AI (chat, modifiche, tag, attività, ricerche), con risposta, modello,
server e fonti. Puoi cercarle, copiare una risposta, aprire la nota,
riprendere una conversazione o eliminarle. La cronologia resta sul
dispositivo (anche nei backup), conserva al massimo le ultime 1.000
richieste e si disattiva o si cancella nelle impostazioni AI.

**Cerca per significato** richiede un modello di embedding (per esempio
`nomic-embed-text`): senza, la scheda spiega cosa configurare.

L'AI non modifica mai le note da sola e può sbagliare: controlla i risultati
e usa le fonti citate per verificarli.

## Calendari iCal (Google, Outlook e altri, in sola lettura)

Puoi vedere nel calendario di Greatnote gli appuntamenti di Google Calendar,
Outlook o qualunque calendario che offra un indirizzo iCal. Non serve
accedere all'account: basta incollare l'indirizzo.

1. **Trova l'indirizzo** (si apre anche da **Dove trovo l'indirizzo iCal?**
   nelle impostazioni):
   - **Google Calendar** (sul web): Impostazioni › scegli il calendario
     nella colonna a sinistra › **Integra calendario** › copia **Indirizzo
     segreto in formato iCal**.
   - **Outlook.com / Microsoft 365** (sul web): Impostazioni › Calendario ›
     Calendari condivisi › **Pubblica un calendario** › scegli il
     calendario e «Può visualizzare tutti i dettagli» › **Pubblica** ›
     copia il collegamento **ICS**.
2. **Aggiungilo:** Impostazioni › Integrazioni › **Calendari iCal**. Incolla
   l'indirizzo e, se vuoi, scegli nome e colore (il nome viene preso dal
   calendario se lo lasci vuoto), poi premi **Aggiungi calendario**.
3. Gli eventi compaiono nel calendario con il loro colore. Si aggiornano
   ogni 15 minuti o con **Sincronizza** e sono visibili anche offline.

Gli eventi sono **in sola lettura**: si modificano nel calendario d'origine.
Puoi comunque aggiungere promemoria. Gli eventi ricorrenti compaiono in ogni
occorrenza, con le eccezioni e gli spostamenti fatti nel calendario
d'origine. Google può impiegare anche qualche ora a pubblicare le modifiche
nell'indirizzo iCal.

**Privacy:** chi conosce l'indirizzo vede il calendario. Greatnote lo
conserva nel portachiavi del sistema, non nel database né nei backup. Se
pensi che sia stato diffuso, reimpostalo nel calendario d'origine (Google:
«Reimposta»; Outlook: annulla la pubblicazione e pubblica di nuovo), poi
rimuovi e aggiungi di nuovo il calendario. **Rimuovi** cancella da Greatnote
gli eventi copiati e i loro promemoria.

## Calendari esterni (Google e Microsoft)

> **Non disponibile in questa versione.** La sezione compare in
> Impostazioni › Integrazioni solo nelle build che includono le
> registrazioni dell'app presso Google o Microsoft (docs/OAUTH_SETUP.md). Le
> istruzioni seguenti valgono per quelle build.

Gli eventi di Google Calendar e di Outlook/Microsoft 365 possono comparire
nel calendario di Greatnote. Sono consultabili anche offline e si aggiornano
ogni 15 minuti, oppure con **Sincronizza**. Puoi crearli, modificarli ed
eliminarli da Greatnote: la modifica arriva subito al calendario d'origine.

- **Colore e origine:** ogni evento esterno ha il colore del suo calendario.
  Aprendolo vedi da quale calendario viene e il pulsante **Apri in …**.
- **Nuovo evento:** nel dialog scegli **Calendario**: "Greatnote (solo su
  questo computer)" oppure un calendario collegato modificabile.
- **Sola lettura:** le occorrenze di eventi ricorrenti e gli eventi di
  calendari in sola lettura (es. festività) si modificano solo nel calendario
  d'origine. Puoi comunque aggiungere promemoria e collegarli alle note.
- **Modifiche in conflitto:** se un evento è stato cambiato altrove dopo
  l'ultima sincronizzazione, Greatnote non lo sovrascrive. Ti chiede di
  sincronizzare e riprovare.
- **Eliminare** un evento esterno lo cancella dal calendario d'origine. Non
  passa dal cestino di Greatnote.

### Collegare l'account

1. **Impostazioni › Integrazioni** › **Collega Google Calendar** o **Collega
   Outlook / Microsoft 365**.
2. Si apre il browser: accedi al tuo account e consenti a Greatnote
   l'accesso al calendario. La pagina finale dice che puoi tornare
   all'app.
3. Viene sincronizzato il calendario principale. Spunta gli altri calendari
   che vuoi vedere; togliendo la spunta i loro eventi spariscono da
   Greatnote, ma non dal fornitore.

Se un pulsante è disattivato, la tua versione di Greatnote non include il
collegamento a quel fornitore. In **Avanzate: registrazione personale
dell'app** puoi usare un'app registrata da te: incolla l'ID client di Google
(app desktop, più il segreto, che finisce nel portachiavi) o l'ID
applicazione Microsoft. I passaggi sono gli stessi descritti per l'editore
in [OAUTH_SETUP.md](OAUTH_SETUP.md). Una registrazione personale ha la
precedenza su quella inclusa.

Con un account aziendale Microsoft l'amministratore IT potrebbe dover
approvare Greatnote. Se Google mostra «app non verificata», l'app inclusa
non ha ancora completato la verifica di Google: puoi proseguire da
«Avanzate».

**Scollegare:** **Scollega** rimuove da Greatnote gli eventi copiati e i loro
promemoria e cancella l'accesso dal portachiavi.

- Con Google l'accesso viene anche revocato presso Google.
- Con Microsoft toglilo anche da account.microsoft.com › **Privacy › App e
  servizi** (account personali) o da myapps.microsoft.com (account di
  lavoro).

**Cosa viene sincronizzato:** gli eventi da 90 giorni prima a 400 giorni
dopo oggi. Titolo, descrizione, orari, giorni interi e link all'evento.
Partecipanti, sale e allegati restano nel calendario d'origine. Gli eventi
esterni non entrano nell'esportazione completa di Greatnote.

## Sincronizzazione tra dispositivi

Greatnote può tenere le stesse note su più computer in due modi, senza
account Greatnote né server:

- **Dropbox:** colleghi il tuo account Dropbox dal browser, senza
  installare il client.
- **Cartella sincronizzata:** una cartella che sincronizzi già con
  Nextcloud, Syncthing, Dropbox, OneDrive o servizi simili.

### Con Dropbox

1. **Impostazioni › Sincronizzazione**, scegli **Dropbox**, dai un nome al
   dispositivo e premi **Collega Dropbox**.
2. Si apre il browser: accedi a Dropbox e premi **Consenti**. La pagina
   finale dice che puoi tornare a Greatnote. Le note vengono subito
   sincronizzate.
3. Ripeti sugli altri computer con lo stesso account Dropbox. Le note già
   presenti vengono unite.

Greatnote vede solo la sua cartella su Dropbox (**Applicazioni ›
Greatnote**), non il resto dei tuoi file. L'accesso resta valido finché non
lo revochi; **Scollega Dropbox** lo revoca e ferma la sincronizzazione, e le
note restano dove sono. Non modificare né cancellare a mano i file in quella
cartella.

### Con una cartella sincronizzata

1. Su ogni computer installa il servizio che sincronizza la cartella (per
   esempio il client Nextcloud o Syncthing) e verifica che la cartella sia
   la stessa ovunque.
2. In Greatnote apri **Impostazioni › Sincronizzazione**, premi **Scegli…**
   e seleziona la cartella. Dai un nome al dispositivo (es. «Portatile») e
   premi **Attiva la sincronizzazione**.
3. Ripeti sugli altri computer scegliendo la stessa cartella. Le note già
   presenti vengono **unite**: nessuna nota viene eliminata.

Cosa succede:

- **Frequenza:** la sincronizzazione avviene ogni 2 minuti, all'avvio e con
  **Sincronizza ora**. In basso a destra, accanto a «Salvato sul
  dispositivo», vedi l'ultimo orario («Sincronizzato 10:42») o un
  eventuale errore; cliccandolo apri le impostazioni.
- **Cosa si sincronizza:** note, taccuini, spazi, tag, allegati (anche il
  testo letto dalle immagini), collegamenti, attività, eventi del
  calendario di Greatnote, promemoria, modelli e cestino.
- **Cosa resta locale:** cronologia delle versioni, «Recenti», impostazioni,
  dati dell'assistente AI e calendari iCal o esterni, da aggiungere su ogni
  computer.
- **Funziona offline:** le modifiche fatte senza rete partono alla prima
  sincronizzazione utile.
- **Nota aperta:** se la nota aperta cambia su un altro computer e non la
  stai modificando, viene aggiornata.

**Modifiche contemporanee.** Se modifichi la stessa nota su due computer
prima che si sincronizzino, nessun testo va perso. Tutti i dispositivi
mostrano la stessa versione; l'altra è conservata nella **cronologia della
nota** (Nota › Informazioni › Cronologia), dove puoi confrontarla e
ripristinarla. La nota compare anche tra le **Modifiche contemporanee**
nelle impostazioni, dove puoi aprirla e segnarla come controllata. Se una
nota viene eliminata su un computer mentre la modifichi su un altro, la
modifica prevale e la nota resta.

**Da sapere:**

- **Riservatezza:** i file nella cartella non sono cifrati, quindi il
  servizio che la ospita può leggerli. Per la massima riservatezza usa
  Syncthing, che sincronizza direttamente tra i tuoi computer, o un tuo
  Nextcloud.
- **Non copiare il database tra computer:** usa la sincronizzazione. Se
  Greatnote trova lo stesso database su due computer si ferma con un avviso;
  disattiva e riattiva la sincronizzazione su uno dei due per dargli una
  nuova identità.
- **Allegati in ritardo:** gli allegati grandi possono arrivare dopo la
  nota. L'impostazione «Allegati in arrivo» li conta e Greatnote li completa
  appena il servizio li ha copiati.
- **Disattivare:** le note restano su ogni computer e nella cartella.

## Integrazioni: server MCP locale

Le app di AI compatibili con MCP (Claude Code, Claude Desktop e altre)
possono cercare e leggere le tue note, senza modificarle.

1. Apri **Impostazioni › Integrazioni** e spunta **Consenti l'accesso alle
   note tramite MCP (sola lettura)**.
2. Per Claude Code copia il comando mostrato (`claude mcp add greatnote --
   … --mcp --data-dir …`) ed eseguilo nel terminale. Per altre app copia la
   configurazione JSON nella loro configurazione dei server MCP.
3. Nell'app di AI chiedi per esempio «cerca nelle mie note di Greatnote il
   verbale di lunedì».

Strumenti disponibili: `search_notes` (stessa sintassi della ricerca:
`tag:`, `notebook:`, `is:pinned`, date…), `read_note`, `list_notebooks` e
`list_tags`. Le note nel cestino e quelle dei taccuini esclusi dall'AI non
sono mai accessibili. **Attività recenti** mostra le ultime richieste
(strumento ed esito, senza testo). Togliendo la spunta l'accesso viene
revocato subito. Per rimuovere anche la configurazione dal client:
`claude mcp remove greatnote`.

Ciò che l'app di AI legge può essere inviato al suo fornitore secondo le
sue condizioni. Greatnote non apre porte di rete: è l'app di AI ad
avviarlo in background.

## Dati

Il database locale si chiama `gna.sqlite3` e vive nella directory dati
assegnata da Tauri (`~/.local/share/dev.greatnote.notes/` su Linux; i dati
della cartella `dev.gna.notes` delle versioni precedenti vengono spostati lì
al primo avvio), insieme alle
cartelle `attachments/` e `backups/`. Il cestino è una cancellazione logica: la cancellazione definitiva avviene
solo da lì, con **Elimina definitivamente** o **Svuota cestino**.
Attività, eventi e promemoria sono nello stesso database: sono inclusi nei
backup e nell'archivio completo. Le impostazioni AI (mai la chiave API) e
l'indice semantico sono nel database e quindi nei backup, ma non
nell'archivio di esportazione.

## Limitazioni

La sincronizzazione richiede una cartella sincronizzata da un servizio
esterno; i file non sono cifrati. L'AI richiede un server di
modelli configurato dall'utente (o Claude Code); la trascrizione di riunioni
non è disponibile.
Promemoria solo ad app aperta; attività e promemoria ricorrenti e vista
oraria del calendario non disponibili; i calendari esterni sono solo in
lettura tramite iCal. La ricerca nelle
attività ignora maiuscole/minuscole solo per lettere senza accenti. Backup interni non cifrati (solo le copie esportate possono esserlo). Il
PDF dipende dal dialog di stampa del sistema. Gli allegati
di testo vengono indicizzati fino a 100.000 caratteri; gli altri tipi sono
ricercabili per nome. La sintassi booleana della ricerca non è supportata.
