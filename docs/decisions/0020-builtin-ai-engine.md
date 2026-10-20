# ADR 0020: Motore AI integrato (llama.cpp)

- **Stato:** accettata
- **Data:** 2026-10-21

## Contesto

Le funzioni AI (ADR 0010) richiedono un server compatibile OpenAI già
installato e configurato, per esempio Ollama. Per molti utenti, in
particolare su Windows, è un ostacolo: bisogna installare un altro
programma, scaricare i modelli da riga di comando e conoscerne i nomi.

Abbiamo valutato tre strade:

1. **`llama-server` come processo separato**, distribuito con l'app.
2. **llama.cpp nel processo dell'app**, con il crate `llama-cpp-2`.
3. **Inferenza in Rust puro** (`candle`).

## Decisione

- **Processo separato (opzione 1).**
  - Riusa il client `OpenAiCompatible` già testato, quindi la logica nuova
    è poca.
  - Se il modello esaurisce la memoria e il server va in crash, l'app
    resta aperta.
  - La build non richiede CMake né un compilatore C++.
- **Versione fissata.** Si usa la build b11397 di llama.cpp per Windows x64
  con Vulkan: accelera sulle GPU NVIDIA, AMD e Intel e ripiega sulla CPU.
  - `scripts/fetch-llama.ps1` la scarica e ne verifica lo SHA-256.
  - `tauri.windows.conf.json` la include nelle risorse dell'installer, in
    `llama/`.
  - Nell'installer finiscono solo `llama-server.exe`, le librerie e le
    licenze.
  - Su Linux il bundle non la include: l'app cerca `llama-server` nel
    `PATH`, oppure nel percorso assoluto indicato da
    `GREATNOTE_LLAMA_SERVER`.
- **Catalogo fisso di modelli** (`src/ai/builtin/catalog.rs`).
  - **Chat:** Gemma 4 E2B (consigliato), Qwen3.5 0.8B, Gemma 4 E4B.
  - **Contesto/embedding:** BGE-M3 (consigliato), Nomic Embed Text v2.
  - **Visione:** Qwen3-VL 2B (consigliato), Gemma 4 E2B con proiettore.
  - Tutti hanno licenza Apache 2.0 o MIT. Sono GGUF ufficiali di
    `ggml-org` su Hugging Face, con architetture supportate dalla build
    b11397.
  - Ogni URL punta a un commit preciso del repository, mai a `main`.
    Dimensione e SHA-256 sono verificati prima di dare al file il nome
    definitivo.
- **Download solo su richiesta.**
  - Nessun modello è nell'installer.
  - Ogni download chiede conferma e mostra la dimensione. Il pulsante
    "Scarica i consigliati" scarica un modello per ruolo.
  - Il download si può interrompere e riprende da dove si era fermato
    (`Range`).
  - Il client accetta solo connessioni `https`.
  - Due download non scrivono mai sullo stesso file.
- **Dove stanno i modelli:** nella cartella dati *locale*
  (`%LOCALAPPDATA%` su Windows, non nel profilo roaming), in `ai-models`.
  Non sono inclusi nei backup né nella sincronizzazione. Un modello scelto
  nelle impostazioni salvate non si può eliminare.
- **Processi.**
  - C'è un server per ogni configurazione (modello, proiettore,
    embedding). Si avvia alla prima richiesta e si ferma dopo 10 minuti di
    inattività, all'uscita dall'app o quando le impostazioni cambiano.
  - Se chat e visione usano lo stesso file (Gemma 4 E2B), un solo processo
    con proiettore serve entrambe.
  - Il server ascolta solo su `127.0.0.1`, su una porta libera, e accetta
    solo richieste con una chiave casuale generata a ogni avvio. La chiave
    passa nell'ambiente (`LLAMA_API_KEY`), non negli argomenti.
  - Sono disattivati la web UI e l'accesso alla rete (`--no-webui`,
    `--offline`).
  - Il primo avvio usa la GPU, lasciando a llama.cpp il calcolo degli
    strati; se fallisce, si riprova solo con la CPU (`--device none`).
  - Il contesto della chat è ricavato dal limite di caratteri delle
    impostazioni (da 8192 a 32768 token).
  - Il ragionamento esplicito è disattivato (`--reasoning-budget 0`) per
    risposte rapide anche su CPU.
- **Impostazioni.** Il provider `builtin` è sempre locale, quindi non
  richiede consenso. I campi `chat_model`, `embedding_model` e
  `vision_model` contengono gli id del catalogo, controllati per ruolo
  dal backend.

## Conseguenze

- L'installer Windows cresce di circa 35 MB compressi (circa 94 MB
  installati). I modelli occupano da 0,5 a 4,6 GB ciascuno.
- La CI deve eseguire `scripts/fetch-llama.ps1` prima di qualunque build
  Windows: le risorse devono esistere già quando compila `tauri-build`.
- Se l'app termina in modo anomalo, un server può restare attivo finché
  non viene chiuso a mano. La chiusura normale lo ferma sempre.
- Per aggiornare llama.cpp o un modello bisogna cambiare versione e hash
  nello script o nel catalogo, e verificare che le architetture siano
  supportate.
- Gli installer non sono firmati (ADR 0019), e nemmeno `llama-server.exe`.
  Alcuni antivirus potrebbero segnalarlo.
