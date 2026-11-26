# ADR 0035: registrazione e trascrizione delle riunioni

- **Stato:** accettata
- **Data:** 2026-11-26

## Contesto

La specifica (P3) chiede la trascrizione delle riunioni con questi
requisiti:
- registrazione solo dopo un'azione esplicita e visibile, con indicatore
  persistente;
- scelta e prova del microfono;
- motore locale o provider configurato;
- riepilogo e azioni;
- consenso dei partecipanti;
- possibilità di cancellare audio e trascrizione;
- nessuna registrazione nascosta.

## Decisione

### Motore di trascrizione

- **Lo stesso motore integrato.** La build b11397 di llama.cpp (ADR 0020)
  accetta l'audio tramite il proiettore multimodale. Gemma 4 E2B, già nel
  catalogo per chat e immagini, capisce l'audio con lo stesso proiettore: chi
  l'ha già scaricato non scarica nulla.
  - Prova reale con voce sintetica (espeak-ng): 75 secondi di audio in
    20 secondi con la GPU (Vulkan), testo fedele salvo qualche nome proprio.
  - llama.cpp segnala il supporto audio come sperimentale. L'interfaccia lo
    dice.
- **Nessun programma in più.** Whisper (whisper.cpp) avrebbe richiesto un
  altro eseguibile per piattaforma, con le sue build e i suoi hash.
- **Ruolo «Audio».** Nuovo ruolo di modello `audio` con impostazione
  `audio_model`.
  - Catalogo: `gemma-4-e2b-audio` consigliato, con gli stessi file della
    visione.
  - Ricerca su Hugging Face: pipeline `audio-text-to-text` e `any-to-any`,
    con proiettore obbligatorio come per la visione.
  - Se audio, immagini e chat usano lo stesso file, un solo processo con il
    proiettore li serve tutti.
- **Altri provider.** Con i server compatibili OpenAI l'audio parte come
  contenuto `input_audio` (WAV) nella chat. Funziona con llama-server e con i
  modelli audio dei servizi che lo supportano. Senza `audio_model` si usa il
  modello della chat. Claude Code non trascrive l'audio e lo dice.
- **Ragionamento.** Il ragionamento resta acceso e separato dal server:
  l'app legge solo il testo della risposta. Con il ragionamento spento
  (`enable_thinking: false`) le prove davano trascrizioni peggiori, e con
  `--reasoning-budget 0` il ragionamento finiva nel testo.

### Registrazione

- **Microfono della webview** (`getUserMedia`), convertito a PCM 16 bit,
  mono, 16 kHz. Il codice (ScriptProcessor) funziona in WebKitGTK, WebView2 e
  nella WebView di Android senza file da autorizzare nella CSP. Nessuna
  libreria audio nativa in più.
- **Permessi.**
  - Il gestore dei permessi della webview concede il microfono solo alla
    finestra principale e solo mentre il pannello della registrazione è
    aperto (`allow_microphone`). Fotocamera e cattura dello schermo sono
    sempre negate.
  - Su Linux WebKitGTK richiede l'impostazione `enable-media-stream`, attivata
    all'avvio.
  - Su Android serve il permesso `RECORD_AUDIO`, chiesto dal sistema la prima
    volta.
- **Consenso e visibilità.**
  - L'avvio richiede di confermare di aver informato i partecipanti.
  - Durante la registrazione il pannello mostra un indicatore rosso con il
    tempo e non si può chiudere; lo stesso indicatore compare nella barra di
    stato della nota.
  - Il pannello non è modale: si possono prendere appunti nella nota.
- **Audio al sicuro.**
  - I campioni sono scritti ogni 3 secondi in `recordings/<id>.pcm`, nella
    cartella dati (permessi 0700). Una chiusura inattesa perde al massimo
    pochi secondi.
  - Le registrazioni rimaste sono elencate nel pannello, per trascriverle,
    esportarle o eliminarle.
  - Durata massima 4 ore.
- **Fuori da backup, sincronizzazione ed esportazioni:** l'audio resta solo
  su questo dispositivo. Si può esportare in un file WAV scelto dall'utente o
  eliminare. Non diventa un allegato: gli allegati accettano 25 MiB, e
  un'ora di WAV ne occupa circa 115.

### Trascrizione

- **Tratti.** L'audio viene diviso in tratti di al massimo 30 secondi,
  tagliati nella pausa più marcata dopo i primi 20. I tratti di solo
  silenzio non vengono inviati, perché i modelli inventano testo dal
  silenzio; vengono scartate anche le risposte del tipo «[silenzio]».
- **Avanzamento e annullamento** con gli stessi lavori annullabili delle
  altre richieste AI.
- **Privacy:**
  - con il motore integrato o un server locale l'audio non lascia il
    dispositivo;
  - con un server remoto valgono il consenso e le regole delle impostazioni
    AI;
  - una nota di un taccuino escluso dall'AI non viene trascritta.
- **Risultato.** La trascrizione viene inserita in fondo alla nota, con un
  titolo con data e ora e un paragrafo per tratto preceduto dal tempo
  (`[mm:ss]`). Il pannello offre «Riassumi» ed «Estrai decisioni e azioni»
  sulla sola trascrizione, con le azioni AI esistenti: anteprima e conferma
  prima di inserire.

## Alternative scartate

- **whisper.cpp:** altro eseguibile da distribuire e verificare per
  piattaforma.
- **Registrazione nativa con `cpal`:** dipendenza audio nativa (ALSA su
  Linux), permessi Android da gestire a parte. La webview offre già
  microfono, scelta del dispositivo e permessi di sistema.
- **Audio come allegato:** limite di 25 MiB e nuovi tipi di file da gestire
  in anteprima, esportazione e sincronizzazione.
- **Riconoscimento dei parlanti:** non supportato dal modello. Da valutare
  con un motore dedicato.

## Conseguenze e limiti

- **Qualità.** Dipende dal modello e dal microfono. Con Gemma 4 E2B è buona
  per l'italiano e l'inglese, ma i nomi propri vanno controllati. Senza GPU
  la trascrizione è molto più lenta della durata dell'audio.
- **Parlanti.** La trascrizione non distingue chi parla.
- **Da verificare con un microfono reale** su ogni piattaforma, in
  particolare WebKitGTK su Linux, che richiede i plugin GStreamer per
  PipeWire o PulseAudio.
