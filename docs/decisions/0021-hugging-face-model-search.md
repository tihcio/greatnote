# ADR 0021: Modelli del motore integrato cercati su Hugging Face

- **Stato:** accettata
- **Data:** 2026-10-21

## Contesto

Il motore integrato (ADR 0020) offre un catalogo fisso di sette modelli,
scritto nel codice. Questo crea due problemi:

- per aggiungere un modello bisogna ricompilare e ridistribuire l'app;
- i colleghi hanno computer diversi (8–64 GB di RAM, con o senza scheda
  video), e ognuno deve poter scegliere il modello adatto al proprio.

L'utente ha scelto di prendere l'elenco direttamente da Hugging Face e di
scaricare i modelli da lì, senza un catalogo pubblicato dall'editore.

## Decisione

- **Ricerca libera su Hugging Face**, separata per ruolo (chat, contesto,
  visione). Si usa l'API pubblica, senza account né token:
  - `/api/models?filter=gguf&sort=downloads` per la ricerca;
  - `/api/models/{repo}` per la revisione corrente (`sha`);
  - `/api/models/{repo}/tree/{sha}?recursive=true` per file, dimensioni e
    SHA-256 (`lfs.oid`).

  Per il contesto la ricerca filtra `feature-extraction` e
  `sentence-similarity`; per la visione `image-text-to-text` e
  `any-to-any`.
- **Stesse garanzie del catalogo curato.**
  - Ogni file è fissato al commit letto in quel momento, mai a `main`.
  - Dimensione e SHA-256 vengono dai metadati LFS di Hugging Face e il
    download li verifica (`store.rs`, invariato).
  - Al download il backend rilegge i dettagli da Hugging Face e non usa
    URL o hash arrivati dall'interfaccia.
- **Le risposte di Hugging Face sono dati non attendibili.** Prima di
  entrare in un URL o in un nome di file vengono controllati:
  - i nomi dei repository (`autore/nome`, caratteri ammessi, niente `..`);
  - i percorsi dei file (caratteri sicuri, al massimo due cartelle);
  - gli hash.

  Le risposte hanno un limite di 16 MB. I repository riservati (`gated`) o
  privati non sono scaricabili.
- **Varianti.**
  - Ogni GGUF è una variante, di solito una quantizzazione (Q4_K_M, Q8_0…).
  - I modelli divisi in più parti (`-00001-of-0000N.gguf`) sono una sola
    variante; quelli incompleti vengono scartati.
  - Sono esclusi i file ausiliari: teste MTP, `imatrix` e `draft-`.
  - Per la visione si usa il proiettore `mmproj` del repository (preferiti
    Q8_0, poi F16, BF16, F32). Senza proiettore il repository non è
    proposto per la visione.
- **Nomi locali** `hf-<12 hex di sha256(repo@revisione)>-<percorso con _>`.
  - Lo stesso nome indica sempre lo stesso contenuto.
  - Le parti successive di un modello diviso mantengono lo schema che
    llama.cpp usa per trovarle accanto alla prima.
- **Registro** `hub-models.json` nella cartella dei modelli.
  - Scritto in modo atomico, con file temporaneo e rinomina.
  - Ogni voce viene ricontrollata alla lettura: una voce manomessa (per
    esempio con un percorso `../`) viene ignorata.
  - Gli id sono `hf:<ruolo>:<autore/repo>:<variante>`. Il ruolo nell'id
    permette di validare le impostazioni senza leggere il disco.
- **Adattezza all'hardware** (`hardware.rs`).
  - **RAM:** `GlobalMemoryStatusEx` su Windows, `/proc/meminfo` su Linux.
  - **Schede video:** quelle elencate da `llama-server --list-devices`,
    cioè le stesse che userà il motore, con un tempo massimo di 20 s.
  - **Schede integrate:** dichiarano come propria una parte della RAM, a
    volte con più memoria libera che totale. Si riconoscono da questo o
    dal nome (Iris, UHD, "Radeon Graphics"…) e non contano come memoria
    video dedicata.
  - **Memoria stimata:** dimensione del file + 20% + 768 MB.
  - **Esito:** *scheda video* se sta nella memoria dedicata; *processore*
    se sta nel 60% della RAM più la memoria video; *al limite* fino
    all'85%; altrimenti *troppo grande*.
  - **Variante consigliata:** la più grande che sta nella scheda video;
    sul processore Q4_K_M o la più grande non oltre essa; mai le varianti
    non quantizzate.
- **Catalogo curato invariato.** Resta come scelta predefinita, con i
  modelli provati con il motore; i suoi id non cambiano, quindi le
  impostazioni salvate restano valide.

## Conseguenze

- I modelli di Hugging Face **non sono provati** con la versione di
  llama.cpp distribuita: un'architettura troppo recente non si avvia.
  - L'interfaccia lo dice nella ricerca e accanto a ogni modello.
  - Se il server non si avvia, l'errore riporta le ultime righe del suo
    registro.
- **Licenze:** si mostra quella indicata nei tag e la si ripete nella
  conferma di download. Alcune non sono open source (per esempio `gemma`,
  `llama3`): verificarle resta compito dell'utente.
- **Privacy:** la ricerca invia a Hugging Face solo il testo cercato, mai
  le note, e parte solo dopo che l'utente ha aperto la ricerca.
- **Limite di Hugging Face:** l'API può limitare le richieste (`429`).
  L'errore viene mostrato e si può riprovare.
- **Stime indicative:** non tengono conto delle altre applicazioni aperte
  né dei driver.
- **Modello aggiornato su Hugging Face:** scaricare di nuovo la stessa
  variante la fissa alla nuova revisione ed elimina i file della
  precedente, se nessun altro modello li usa.
