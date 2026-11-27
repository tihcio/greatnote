# ADR 0036: trascrizione delle riunioni con Whisper (whisper.cpp)

- **Stato:** accettata
- **Data:** 2026-11-27
- **Aggiorna:** ADR 0035 (il motore di trascrizione)

## Contesto

La prima versione della trascrizione (ADR 0035) usava modelli linguistici
multimodali tramite llama.cpp (Gemma 4 E2B/E4B). Anche dopo aver corretto la
cattura audio, l'utente ha giudicato il testo inutilizzabile per una
riunione vera: parole storpiate, frasi perse.

Per il parlato serve un motore specializzato. Whisper è il riferimento
multilingue con un buon italiano. whisper.cpp è dagli autori di llama.cpp,
usa la stessa base (ggml), distribuisce eseguibili già compilati per Linux e
Windows e funziona senza Python.

### Prove sulla stessa lettura di 63 secondi (voce reale, microfono del portatile)

Tutte con il solo processore (i7-12700H):

| Configurazione | Tempo | Esito |
| --- | --- | --- |
| Gemma 4 E2B | ~40 s* | Parole sbagliate (caldarroste, Montblanc, «saper scegliere») |
| Gemma 4 E4B | 49 s | Meglio, ma ancora errori e accordi sbagliati |
| Whisper turbo, solo lingua | 45 s | Testo buono ma senza punteggiatura né maiuscole |
| + suggerimento iniziale italiano | 48 s | Punteggiatura corretta; un pezzo saltato, «Grazie a tutti» inventato in fondo |
| + VAD Silero | 39 s | Niente testo inventato |
| **Pipeline dell'app** (turbo, suggerimento, VAD, audio ripulito) | **27 s** | **Testo completo, poche parole rare sbagliate** |
| Whisper large-v3 + VAD | 78 s | Il più preciso |

\* con la scheda video in parte occupata da processi orfani (vedi ADR 0020).

Su una riunione reale di 11 minuti, con più persone, Whisper turbo ha
prodotto un testo da rileggere ma utilizzabile, in circa 10 minuti.
Restano storpiati i nomi propri e le sigle, e indirizzi email e numeri
dettati lettera per lettera.

## Decisione

- **Motore:** `whisper-cli` di whisper.cpp, build b5907, con impronta
  fissata.
  - `scripts/fetch-whisper.sh` (Linux, `whisper-bin-ubuntu-x64.tar.gz`) e
    `scripts/fetch-whisper.ps1` (Windows, `whisper-bin-x64.zip`) lo
    preparano in `src-tauri/resources/whisper`, che i pacchetti copiano in
    `whisper/` accanto all'app. Entrano solo l'eseguibile, le librerie
    whisper/ggml e la licenza.
  - La cartella è separata da quella di llama.cpp, perché le due build
    usano versioni diverse delle librerie ggml.
  - Ricerca dell'eseguibile: risorse dell'app, poi `GREATNOTE_WHISPER_CLI`,
    poi il `PATH`.
- **Un processo per registrazione, non un server.** Niente porte né chiavi.
  - Avanzamento dalle righe `--print-progress`, risultato in JSON con i
    tempi di ogni frase (`--output-json`).
  - Annullare chiude il processo, che come i server di llama.cpp si chiude
    anche se l'app termina in modo anomalo.
  - Vengono usati tutti i core del processore meno due.
- **Modelli nel catalogo**, con un nuovo campo `engine`
  (`llama`/`whisper`) e il file VAD come accessorio. Download, ripresa,
  verifica dell'impronta ed eliminazione sono gli stessi degli altri
  modelli.
  - **Whisper large-v3 turbo**, quantizzato q5_0 (574 MB): consigliato.
  - **Whisper large-v3**, quantizzato q5_0 (1,1 GB): più preciso e più
    lento.
  - **VAD Silero v6.2.0** (0,9 MB), scaricato con il modello scelto.

  Tutti dal repository ufficiale `ggerganov/whisper.cpp`, il VAD da
  `ggml-org/whisper-vad`, con revisione fissata. Licenza MIT. I file `.bin`
  sono ammessi come i `.gguf`; i modelli Whisper esistono solo nel catalogo
  curato e non passano mai da llama-server.
- **Parametri tarati con le prove:**
  - lingua della riunione scelta nel pannello (italiano, inglese o
    riconoscimento automatico);
  - VAD sempre attivo, per non inventare testo sui silenzi;
  - suggerimento iniziale che chiede punteggiatura e maiuscole;
  - campo facoltativo **«Nomi e termini»** (persone, aziende, prodotti,
    sigle; al massimo 400 caratteri, ripuliti), aggiunto al suggerimento:
    è il modo più efficace di far scrivere giusti i nomi propri. Lingua e
    termini restano salvati con la registrazione, per ritrascriverla.
- **Audio.** Il WAV passato a Whisper è ricavato dalla registrazione
  ripulita dai buchi della cattura (ADR 0035), scritto accanto alla
  registrazione ed eliminato subito dopo, anche in caso di errore o
  annullamento.
- **Gemma resta disponibile** per l'audio, non più consigliato, insieme ai
  server compatibili OpenAI (`input_audio`). Whisper si usa quando il
  modello audio scelto è un modello Whisper. Basta che l'AI sia attiva:
  nessun dato esce dal dispositivo, quindi non serve il consenso per i
  server remoti. Le note dei taccuini esclusi non vengono trascritte.
- **Correzione e verbale** restano ai modelli di chat: «Riassumi» ed
  «Estrai decisioni e azioni» lavorano sulla trascrizione, con anteprima.

## Alternative scartate

- **faster-whisper o WhisperX (Python):** prestazioni ottime, ma Python e
  le sue dipendenze andrebbero distribuiti con l'app; il resto dell'app ne
  fa a meno.
- **Riconoscimento di chi parla con pyannote:** richiede Python, un
  account Hugging Face e l'accettazione di condizioni d'uso per modello.
  Da rivalutare con una soluzione in C/C++.
- **`whisper-server`:** aggiunge porta e autenticazione senza vantaggi per
  un lavoro per registrazione.
- **Accelerazione con la scheda video:** le release di whisper.cpp non
  hanno build Vulkan. Va compilata nella CI (CMake e Vulkan SDK):
  passo successivo.
- **Voxtral Mini 3B** (tramite llama.cpp): rifiuti e testo inventato
  (ADR 0035).

## Conseguenze

- **Tempi.** Sul processore la trascrizione dura circa quanto la riunione
  con turbo e circa il doppio con large-v3. L'interfaccia mostra la
  percentuale e si può annullare.
- **Dimensioni.** I pacchetti crescono di circa 20 MB (Linux) per
  whisper-cli e le sue librerie; i modelli si scaricano solo su richiesta.
- **Limiti.** La trascrizione non distingue chi parla; nomi propri, sigle
  e dettature lettera per lettera vanno ricontrollati.
