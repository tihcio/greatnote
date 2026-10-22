# ADR 0019: Installer Windows

- **Stato:** accettata
- **Data:** 2026-10-20

## Contesto

L'app deve essere distribuita ai colleghi su Windows. Il dominio e i comandi
Tauri sono già portabili: i permessi Unix sono applicati solo con
`cfg(unix)`, il portachiavi usa Credential Manager e le notifiche usano il
plugin Tauri. Mancavano la configurazione del bundle, le icone `.ico` e una
build riproducibile.

Sul PC di sviluppo Windows 11 è attivo Smart App Control. Questo blocca gli
script di build non firmati compilati da Cargo (`os error 4551`), quindi in
locale non si può compilare.

## Decisione

- **Configurazione separata.** `src-tauri/tauri.windows.conf.json` viene
  unito a `tauri.conf.json` solo su Windows. In questo modo i bundle Linux
  (`deb`, `appimage`) non cambiano.
- **Due installer.**
  - **NSIS** con `installMode: currentUser`. È quello da distribuire ai
    colleghi: non chiede permessi di amministratore e offre la scelta tra
    italiano e inglese.
  - **MSI** (WiX), per l'installazione gestita dall'IT.
- **WebView2** con `downloadBootstrapper` silenzioso. L'installer resta
  piccolo, e WebView2 è già presente su Windows 10/11 aggiornati.
- **Build su GitHub Actions** (`windows-installer.yml`, poi `installers.yml` con ADR 0024) con un runner
  `windows-latest`. Il workflow parte a mano o con un tag `v*`, esegue
  `cargo test --workspace` prima del bundle e pubblica gli installer come
  artefatti. Non crea release pubbliche.
- **Gli ID OAuth** arrivano dai segreti del repository e sono facoltativi.
  Se sono vuoti, le integrazioni risultano non configurate.

## Conseguenze

- Gli installer non sono firmati. SmartScreen mostra un avviso e Smart App
  Control può bloccare l'installazione. Per firmarli serve un certificato
  di firma del codice (ad esempio Azure Trusted Signing o un certificato
  OV/EV). La firma va configurata con `bundle.windows.signCommand` e con
  segreti protetti, non nel repository.
- Non c'è ancora un aggiornamento automatico. Per aggiornare si installa
  la nuova versione sopra la precedente. I dati in
  `%APPDATA%\dev.greatnote.notes` restano e vengono migrati all'avvio.
- La verifica funzionale su un PC Windows pulito resta aperta in ROADMAP
  (Fase 8).
