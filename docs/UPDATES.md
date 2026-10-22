# Aggiornamenti in-app e pubblicazione delle versioni

Greatnote installato dall'installer Windows o dai pacchetti Linux
(AppImage, `.deb`, `.rpm`) cerca da solo le versioni nuove, le scarica in
background, ne verifica la firma e le installa alla chiusura dell'app
(oppure subito con **Riavvia e aggiorna**; per `.deb` e `.rpm` solo così,
vedi sotto). Decisione e
motivazioni: [ADR 0023](decisions/0023-in-app-updates.md).

## Come funziona

1. All'avvio (dopo un minuto) e poi ogni 6 ore l'app legge
   `https://github.com/tihcio/greatnote/releases/latest/download/latest.json`.
   La richiesta contiene solo versione e sistema operativo.
2. Se la versione indicata è più recente di quella installata, scarica
   l'installer NSIS dalla release e ne verifica la firma minisign con la
   chiave pubblica contenuta nell'app (`plugins.updater.pubkey` in
   `src-tauri/tauri.conf.json`). La firma deve indicare la stessa versione
   annunciata (`requireSignedVersion`): un `latest.json` alterato non può
   far installare una versione vecchia.
3. Il pacchetto verificato resta in memoria. Nella barra laterale compare
   **Aggiornamento X pronto**; in Impostazioni › Aggiornamenti si leggono le
   novità e si può installare subito. Altrimenti l'installer parte, in
   modalità passiva (solo barra di avanzamento), quando si chiude l'app.
   **Linux:** l'app sceglie la voce di `latest.json` del proprio formato
   (`linux-x86_64-appimage`, `-deb`, `-rpm`; il formato è scritto nel
   programma al momento del pacchetto). L'AppImage viene sostituita sul
   posto alla chiusura, senza password. `.deb` e `.rpm` si installano con
   `pkexec dpkg -i` / `pkexec rpm -U`, che chiedono la password di
   amministratore: per questo solo con **Riavvia e aggiorna**, mai alla
   chiusura. Se la password viene annullata l'aggiornamento resta pronto.
4. L'installer sostituisce i programmi e non tocca la cartella dati. Se la
   nuova versione cambia lo schema del database, all'avvio viene creato il
   backup `greatnote-premigration-…` prima della migrazione.

**Repository rinominato (2026-10-22).** Fino alla 0.1.1 l'indirizzo era
`tihcio/greatnote-win`. Le installazioni esistenti continuano a leggerlo e
GitHub le reindirizza al nuovo nome `tihcio/greatnote`. Il
reindirizzamento smette di funzionare se si crea un nuovo repository
chiamato `greatnote-win`: quel nome non va riutilizzato.

Gli aggiornamenti automatici si disattivano in Impostazioni ›
Aggiornamenti; **Cerca aggiornamenti** funziona comunque. Nella versione di
sviluppo (`npm run tauri:dev`) sono disattivati.

## Configurazione una tantum

### 1. Repository pubblico

L'app scarica gli aggiornamenti senza credenziali, quindi le release del
repository devono essere pubbliche: GitHub › Settings › General › Danger
Zone › **Change visibility** → Public. Prima di farlo verifica che nella
cronologia git non ci siano segreti (`git log -p | grep -iE "secret|token|key"`
come primo controllo) e che gli ID OAuth arrivino solo dai segreti del
repository.

### 2. Chiave di firma

La coppia di chiavi è stata generata il 2026-10-21 in
`%USERPROFILE%\.tauri\` (fuori dal repository):

| File | Contenuto | Dove deve stare |
| --- | --- | --- |
| `greatnote-updater.key` | chiave privata cifrata | segreto `TAURI_SIGNING_PRIVATE_KEY` + backup |
| `greatnote-updater.password` | password della chiave | segreto `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` + backup |
| `greatnote-updater.key.pub` | chiave pubblica | già in `tauri.conf.json` |

1. In GitHub › Settings › Secrets and variables › Actions crea i due
   segreti incollando il **contenuto** dei file (non il percorso).
2. Salva chiave privata e password in un gestore di password o su un
   supporto esterno cifrato. **Se la chiave privata si perde, le
   installazioni esistenti non potranno più aggiornarsi da sole**: andrà
   pubblicata una versione con una nuova chiave pubblica e installata a mano.
3. Dopo il backup puoi eliminare `greatnote-updater.password` dal disco.

Se la chiave privata viene compromessa: genera una nuova coppia
(`npx tauri signer generate -w %USERPROFILE%\.tauri\greatnote-updater.key`),
aggiorna `pubkey` e i segreti, pubblica una versione e avvisa gli utenti
che quella versione va installata a mano.

## Pubblicare una versione

1. Aggiorna la versione (stessa stringa) in `Cargo.toml`,
   `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` e `package.json`, e
   la sezione del `CHANGELOG.md`. Segui la
   [checklist di rilascio](RELEASE_CHECKLIST.md).
2. Crea e invia il tag:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. Il workflow **Installers** (`installers.yml`):
   - compila e firma gli installer (`--config src-tauri/tauri.updater.conf.json`);
   - controlla che il tag corrisponda alla versione e che la firma contenga
     la versione, rifirmando l'installer se manca
     (`scripts/updater-manifest.ps1`);
   - crea una release **in bozza** con installer `.exe`, firma `.sig`,
     `.msi` e `latest.json`;
   - nel job Linux, compila `.deb`, `.rpm` e AppImage, li rinomina
     (`Greatnote.x86_64.rpm`, …), li firma con la versione vincolata e
     aggiunge le voci `linux-x86_64-*` a `latest.json`
     (`scripts/updater-manifest-linux.sh`), poi carica pacchetti, firme e
     manifest nella stessa bozza.
4. Controlla la bozza in GitHub › Releases, scrivi le note e premi
   **Publish release**. Solo da quel momento le app installate la trovano.

Il campo `notes` di `latest.json` (mostrato nell'app) contiene
«Greatnote X.Y.Z»; per note più ricche modifica `latest.json` nella bozza
prima di pubblicarla, senza toccare `version`, `url` e `signature`.

## Limiti

- **Linux fino alla 0.1.2:** le release precedenti non avevano le voci
  Linux in `latest.json`, quindi quelle installazioni vedono «Non ci sono
  aggiornamenti automatici per questo tipo di installazione» finché non
  esce una versione con le voci Linux; da lì si aggiornano da sole.
- **`.deb`/`.rpm`:** serve un agente polkit (presente in KDE e GNOME) per
  la richiesta di password di `pkexec`; senza, il plugin prova `zenity` o
  `kdialog` e infine `sudo` nel terminale.
- **Installer non firmati con un certificato di firma del codice:** la firma
  minisign protegge l'aggiornamento, ma SmartScreen può mostrare un avviso
  per l'installer.
- **Server MCP attivo:** se un client AI tiene aperto `greatnote --mcp`,
  l'installer può chiedere di chiudere l'app; chiudi il client e riprova.
- Il pacchetto scaricato è in memoria: se l'app si chiude in modo anomalo,
  viene riscaricato al controllo successivo.
