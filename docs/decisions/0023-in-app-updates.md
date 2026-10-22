# ADR 0023: Aggiornamenti in-app firmati

- **Stato:** accettata
- **Data:** 2026-10-21

## Contesto

Ogni nuova versione va oggi scaricata e installata a mano. L'utente vuole
che l'app cerchi le versioni nuove e le installi da sola. Vincoli:

- nessuna credenziale nell'app: chiunque potrebbe estrarla;
- un aggiornamento non verificato è un'esecuzione di codice remoto;
- un utente che sta scrivendo non deve perdere testo;
- il repository `tihcio/greatnote-win` era privato.

## Decisione

- **Plugin ufficiale `tauri-plugin-updater` 2**, senza la funzionalità
  `zip`: gli aggiornamenti sono gli installer stessi (`createUpdaterArtifacts:
  true`), e così l'estrazione di archivi, con vulnerabilità note in zip 4.x,
  non viene compilata. Il frontend non ha i permessi del plugin: usa quattro
  comandi Rust (`get_update_status`, `check_for_updates`, `set_auto_update`,
  `install_update_now`).
- **Release pubbliche su GitHub** (scelta dell'utente: il repository diventa
  pubblico). Endpoint fisso
  `releases/latest/download/latest.json`, solo `https`.
- **Firma minisign** con una chiave generata sul PC dell'utente, fuori dal
  repository; la chiave privata e la password sono segreti di GitHub
  Actions. `requireSignedVersion: true`: la firma deve indicare la versione
  annunciata, così un `latest.json` alterato non può forzare un ritorno a
  una versione vecchia firmata. Lo script `scripts/updater-manifest.ps1`
  verifica la firma e, se manca la versione, rifirma con `--app-version`.
- **Comportamento scelto dall'utente:** controllo un minuto dopo l'avvio e
  ogni 6 ore, download e verifica in background, installazione alla
  chiusura dell'app (installer NSIS in modalità passiva, senza riavvio)
  oppure subito con «Riavvia e aggiorna», che salva prima la bozza aperta.
  Disattivabile; il controllo manuale resta disponibile.
- **Pubblicazione controllata:** il workflow crea la release in bozza; gli
  utenti la ricevono solo quando qualcuno la pubblica.
- Nelle build di sviluppo gli aggiornamenti sono disattivati.

## Alternative scartate

- Token di lettura nell'app per il repository privato: estraibile, quindi
  equivale a renderlo pubblico senza poterlo revocare in sicurezza.
- Repository pubblico separato solo per le release: proposto, scartato
  dall'utente per semplicità.
- Installazione immediata con riavvio automatico: interrompe chi scrive.

## Conseguenze

- Perdere la chiave privata blocca gli aggiornamenti automatici delle
  installazioni esistenti (procedura in `docs/UPDATES.md`).
- Solo Windows riceve aggiornamenti; le installazioni Linux mostrano che non
  sono disponibili.
- Gli installer restano senza certificato di firma del codice (SmartScreen).

## Aggiornamento: Linux (2026-10-22)

- Il plugin (2.14) aggiorna AppImage, `.deb` e `.rpm` cercando in
  `latest.json` la voce `linux-x86_64-<formato>`. Il job Linux del workflow
  firma i pacchetti rinominati con `tauri signer sign --app-version` (stessa
  chiave e stessa versione vincolata di Windows) e aggiunge le voci al
  manifest della bozza (`scripts/updater-manifest-linux.sh`).
- L'AppImage si installa alla chiusura come su Windows. `.deb` e `.rpm`
  passano da `pkexec` e chiedono la password: si installano solo con
  «Riavvia e aggiorna» (`installs_on_exit` falso nello stato), e se la
  richiesta viene annullata il pacchetto resta pronto.

