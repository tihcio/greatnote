# ADR 0024: Un solo repository per Linux e Windows, motore integrato anche su Linux

- **Stato:** accettata
- **Data:** 2026-10-22

## Contesto

Dal 20/10 l'app esisteva in due repository senza storia git in comune:
`tihcio/Greatnote` (Linux) e `tihcio/greatnote-win` (Windows), nato da una
copia del primo allo stato "fase 7". In due giorni si erano già prodotti:

- correzioni presenti da una sola parte. Per esempio, la sincronizzazione
  che non blocca il database durante le chiamate di rete esisteva solo su
  Linux;
- lo stesso problema risolto in due modi: motore AI con llama.cpp su
  Windows (ADR 0020) e Ollama incluso nel pacchetto su Linux;
- numeri di ADR in conflitto (0019 e 0020).

## Decisione

- **Un solo repository**, `tihcio/greatnote` (rinominato da
  `greatnote-win`), per tutte le piattaforme. Le differenze stanno solo
  in:
  - `tauri.windows.conf.json` e `tauri.linux.conf.json`;
  - gli script di build (`fetch-llama.ps1`, `fetch-llama.sh`);
  - poche sezioni `cfg` nel codice.
- **Dal repository Linux** è stata portata la correzione della
  sincronizzazione (tre fasi, rete fuori dal blocco del database, test
  `network_calls_never_hold_the_database_lock`), insieme al test sulla
  CSP. La barra laterale con lo stato della sincronizzazione esisteva in
  entrambi: si è tenuta quella di Windows.
- **Motore AI identico su Linux** (ADR 0020). `scripts/fetch-llama.sh`
  prepara la stessa build b11397 di llama.cpp nella variante
  `ubuntu-vulkan-x64`:
  - SHA-256 fissato;
  - solo `llama-server` e le librerie che carica;
  - nessun link simbolico: ogni libreria prende il nome del suo SONAME.

  `tauri.linux.conf.json` la include nei pacchetti `.deb`, `.rpm` e
  AppImage. Lo script viene eseguito prima di `tauri build` e `tauri dev`.
  Vulkan funziona sulle GPU NVIDIA, AMD e Intel. llama.cpp sceglie da solo
  la GPU dedicata e ignora quella integrata.
- **Dipendenze dei pacchetti**: `vulkan-loader` e `openssl-libs` (RPM);
  `libvulkan1` e `libssl3` (deb).
- Senza la risorsa (cargo check/clippy/test, CI), `build.rs` crea un
  segnaposto. L'app cerca allora `llama-server` nel `PATH` o in
  `GREATNOTE_LLAMA_SERVER`.
- **Ollama incluso nel pacchetto Linux è abbandonato.** Con la variante
  CUDA pesava circa 1,2 GB contro gli 83 MB di llama.cpp, e duplicava il
  catalogo dei modelli. Ollama resta utilizzabile come server esterno
  compatibile OpenAI. Il lavoro resta nella storia del vecchio repository
  Linux.
- **Aggiornamenti in-app** (ADR 0023): l'endpoint punta al nuovo nome. Le
  installazioni 0.1.0/0.1.1 usano ancora `greatnote-win` e GitHub le
  reindirizza, purché nessun nuovo repository riprenda quel nome. Su Linux
  gli aggiornamenti in-app non sono ancora attivi.

## Conseguenze

- Il pacchetto RPM pesa poche decine di MB compressi. Con la RTX 3070 di
  prova il modello è stato caricato tutto in VRAM con Vulkan (210 token/s
  con Qwen3.5 0.8B).
- Ogni correzione si fa una volta. La CI controlla già Linux e Windows.
- Il vecchio repository `tihcio/Greatnote` va archiviato o rinominato:
  serve anche a liberare il nome `greatnote`.
