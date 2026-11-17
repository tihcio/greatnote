# Microsoft Store (pacchetto MSIX)

Gli installer `.exe`/`.msi` di GitHub non sono firmati con un certificato
di firma del codice, quindi SmartScreen e Smart App Control possono
bloccarli. Il pacchetto MSIX pubblicato sul Microsoft Store è firmato da
Microsoft dopo la certificazione e si installa senza avvisi. La decisione
è descritta nell'[ADR 0029](decisions/0029-microsoft-store-msix.md).

## Cosa contiene il pacchetto

- lo stesso `greatnote-desktop.exe` degli installer, con la cartella `llama`
  del motore AI integrato;
- i loghi in `src-tauri/msix/Assets` (generati da `src-tauri/icons/icon.png`);
- il manifest `src-tauri/msix/AppxManifest.xml`.

Lo crea `scripts/build-msix.ps1` con `makepri` e `makeappx` del Windows
SDK; il workflow *Installers* lo produce a ogni esecuzione come artefatto
`greatnote-windows-msix`. La CI (`msix-manifest`) impacchetta un MSIX di
prova a ogni push, così un manifest non valido viene scoperto subito.

## Differenze dalla versione installata da GitHub

| | Installer GitHub | Microsoft Store |
|---|---|---|
| Firma | nessuna | Microsoft |
| Aggiornamenti | updater interno (ADR 0023) | dallo Store; l'updater interno è disattivato |
| Cartella dati | `%APPDATA%\dev.greatnote.notes` | la stessa |
| Modelli AI e cache | `%LOCALAPPDATA%\dev.greatnote.notes` | la stessa |
| Disinstallazione | i dati restano | i dati restano |
| Server MCP | percorso di `greatnote-desktop.exe` | alias `%LOCALAPPDATA%\Microsoft\WindowsApps\greatnote.exe` |
| Notifiche | ID `dev.greatnote.notes` | ID dell'app del pacchetto |

L'app riconosce da sola se gira dentro il pacchetto (`src-tauri/src/msix.rs`).

Le cartelle dati sono escluse dalla virtualizzazione dei file di MSIX
(capability ristretta `unvirtualizedResources`): senza, Windows sposterebbe
le scritture in una cartella privata del pacchetto, le note non sarebbero
visibili all'installazione da GitHub e verrebbero cancellate disinstallando
l'app dallo Store. Su Windows 11 l'esclusione vale solo per le tre cartelle
dell'app; su Windows 10 per tutta `AppData`.

Le due installazioni possono convivere, ma è meglio usarne una sola: se
sono aperte insieme usano lo stesso database.

## Pubblicazione (prima volta)

1. **Account sviluppatore** su <https://storedeveloper.microsoft.com>:
   per gli sviluppatori individuali è gratuito; serve la verifica
   dell'identità.
2. **Partner Center › App e giochi › Nuovo prodotto › App MSIX o PWA**:
   prenota il nome. Fatto: «Greatnote», Store ID `9P7KJXLWGP5P`,
   <https://apps.microsoft.com/detail/9P7KJXLWGP5P> (attivo dopo la prima
   certificazione).
3. **Gestione prodotto › Identità prodotto**: i tre valori sono già in
   `scripts/build-msix.ps1`:

   | Partner Center | Valore |
   |---|---|
   | `Package/Identity/Name` | `TizianoAngeli.Greatnote` |
   | `Package/Identity/Publisher` | `CN=F1EEA09B-E60D-40CC-8D0B-AF312F4E74C3` |
   | `Package/Properties/PublisherDisplayName` | `TizianoAngeli` |

   Non sono segreti, ma il pacchetto deve contenerli esattamente: con
   valori diversi Partner Center rifiuta il caricamento. Le variabili
   d'ambiente `MSIX_IDENTITY_NAME`, `MSIX_PUBLISHER` e
   `MSIX_PUBLISHER_DISPLAY_NAME` li sovrascrivono, se mai servisse.
4. **Crea una release** (tag `vX.Y.Z`) e scarica l'artefatto
   `greatnote-windows-msix` dall'esecuzione del workflow *Installers*.
   La versione del pacchetto è `X.Y.Z.0` e deve crescere a ogni invio.
5. **Nuovo invio** in Partner Center:
   - *Prezzi e disponibilità*: gratuita, mercati a scelta;
   - *Proprietà*: categoria Produttività; URL dell'informativa sulla
     privacy, ad esempio
     `https://github.com/tihcio/greatnote/blob/main/src-tauri/installer/license-and-privacy.txt`;
   - *Classificazioni per età*: questionario;
   - *Pacchetti*: carica il file `.msix`;
   - *Elenchi dello Store* (italiano e inglese): descrizione e almeno una
     schermata (le immagini di `tests/ui` possono servire da base);
   - *Opzioni di invio › Capability ristrette*: motivazione, ad esempio:

     > runFullTrust: Greatnote is a desktop application built with Tauri
     > (Rust and WebView2); it runs as a full-trust Win32 process and
     > starts its bundled local AI engine (llama-server) as a child process.
     >
     > unvirtualizedResources: Greatnote stores the user's notes,
     > attachments and backups in %APPDATA%\dev.greatnote.notes and local
     > AI models in %LOCALAPPDATA%\dev.greatnote.notes. These folders are
     > excluded from virtualization so that the notes are shared with the
     > non-Store installation of the same app and are not deleted when the
     > app is uninstalled. No other location is excluded.
6. **Certificazione**: di solito da qualche ora a qualche giorno. Gli invii
   successivi richiedono solo il nuovo pacchetto e le note di versione.

## Prova in locale

Serve il Windows SDK (makeappx, makepri, signtool) e un'app compilata:

``` powershell
npx tauri build
./scripts/build-msix.ps1 -TestCertificate
```

`-TestCertificate` firma il pacchetto con un certificato autofirmato
(`target/release/bundle/msix/Greatnote-test.cer`). Per installarlo,
da PowerShell come amministratore:

``` powershell
Import-Certificate -FilePath target\release\bundle\msix\Greatnote-test.cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople
Add-AppxPackage target\release\bundle\msix\Greatnote_X.Y.Z.0_x64.msix
```

Dopo la prova rimuovi il pacchetto (`Get-AppxPackage *Greatnote* |
Remove-AppxPackage`) e il certificato da *Persone attendibili*.

## Limiti

- WebView2 deve essere presente: lo è su Windows 11 e su Windows 10
  aggiornato. Il pacchetto non può installarlo.
- Solo x64 (come gli installer).
- I binari di llama.cpp sono inclusi così come sono: la firma di
  Microsoft copre il pacchetto intero.
