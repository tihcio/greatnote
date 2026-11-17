# ADR 0029: pacchetto MSIX per il Microsoft Store

- **Stato:** accettata
- **Data:** 2026-11-17

## Contesto

Gli installer Windows non sono firmati: Smart App Control ne impedisce
l'avvio e SmartScreen mostra un avviso. La firma gratuita con SignPath
Foundation (ADR 0028) è stata respinta perché il progetto non ha ancora
abbastanza visibilità pubblica. I certificati commerciali costano, e Azure
Artifact Signing non è disponibile per i privati in Europa.

Da settembre 2025 l'account sviluppatore individuale del Microsoft Store è
gratuito, e lo Store firma i pacchetti MSIX dopo la certificazione.

## Decisione

Distribuire su Windows anche un pacchetto MSIX tramite il Microsoft Store,
accanto agli installer di GitHub.

- Tauri genera solo NSIS e MSI: `scripts/build-msix.ps1` impacchetta lo
  stesso `greatnote-desktop.exe` con `makepri`/`makeappx` del Windows SDK. Un solo
  eseguibile per entrambe le distribuzioni.
- L'app riconosce l'identità del pacchetto (`GetCurrentPackageFamilyName`,
  `src-tauri/src/msix.rs`) e in quel caso:
  - disattiva l'updater interno: aggiorna lo Store;
  - invia le notifiche con l'AppUserModelID del pacchetto, perché Windows
    rifiuta quelle con l'identificatore usato dal plugin;
  - indica ai client MCP l'alias di esecuzione `greatnote.exe`.
- Le cartelle dati sono escluse dalla virtualizzazione (capability ristretta
  `unvirtualizedResources`, "flexible virtualization" di Windows 11, con
  `desktop6:FileSystemWriteVirtualization` per Windows 10). Così le note
  sono le stesse dell'installazione da GitHub e restano dopo la
  disinstallazione, come richiesto dalla specifica (sezione 18).
- L'identità del prodotto assegnata da Partner Center
  (`TizianoAngeli.Greatnote`, Store ID 9P7KJXLWGP5P) è scritta in
  `scripts/build-msix.ps1`: non è un segreto e non cambia.
- Il caricamento su Partner Center è manuale: niente credenziali dello
  Store nel CI.

## Alternative scartate

- **Disattivare tutta la virtualizzazione anche su Windows 11:** più
  semplice, ma toglie l'isolamento a tutto `AppData`.
- **Lasciare la virtualizzazione:** le note finirebbero in una cartella del
  pacchetto, invisibili all'installazione da GitHub e cancellate con la
  disinstallazione.
- **Strumenti di terze parti per l'MSIX di Tauri:** dipendenza in più per
  un passaggio che il Windows SDK già offre.

## Conseguenze

- Lo Store deve accettare la motivazione per `unvirtualizedResources` e
  `runFullTrust`.
- Ogni versione passa la certificazione dello Store prima di arrivare agli
  utenti.
- Gli installer di GitHub restano non firmati; lo Store diventa il canale
  consigliato su Windows.
- Se entrambe le versioni sono installate condividono il database.
