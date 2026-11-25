# ADR 0031 — Modalità in background sul desktop

- Stato: accettata
- Data: 2026-11-25

## Contesto

Sul desktop i promemoria partono dallo scheduler dell'app (un thread nel
processo Rust): con Greatnote chiuso non arrivano, e all'avvio successivo
compaiono come «persi mentre l'app era chiusa». Su Android il sistema
consegna già le notifiche ad app chiusa (`reminder_alarms.rs`).

## Decisione

- **Modalità in background facoltativa** (Impostazioni › Generali), spenta
  per impostazione predefinita e salvata nella tabella `settings` (chiave
  `background`, non sincronizzata: riguarda questo computer).
- Con la modalità attiva:
  - un'**icona nell'area di notifica** (funzione `tray-icon` di Tauri) con
    «Apri Greatnote» ed «Esci da Greatnote»; il clic sinistro apre la
    finestra;
  - **chiudere la finestra principale la nasconde** dopo che il frontend ha
    salvato la bozza: lo scheduler continua a inviare le notifiche. Le note
    aperte in una finestra separata si chiudono come sempre;
  - **«Esci»** (menu File o icona) chiude davvero: l'icona chiede al
    frontend di salvare e chiudere (evento `quit-requested`) e dopo 5
    secondi chiude comunque (le bozze sono già nel database).
- **Avvio all'accesso** facoltativo, con la finestra nascosta (argomento
  `--background`):
  - Linux: file `~/.config/autostart/dev.greatnote.notes.desktop`; per
    un'AppImage il percorso è quello dell'AppImage (`$APPIMAGE`), non del
    punto di montaggio temporaneo. La voce viene riscritta a ogni avvio, così
    segue un'AppImage spostata;
  - Windows: valore `Greatnote` in `HKCU\…\CurrentVersion\Run`;
  - pacchetto del Microsoft Store: non disponibile, perché la voce del
    registro non vale e lo `StartupTask` del manifest non passa argomenti
    (la finestra si aprirebbe).
- **Una sola istanza** (plugin ufficiale `tauri-plugin-single-instance`):
  aprire di nuovo Greatnote mostra la finestra di quella già attiva invece
  di avviare un secondo processo sullo stesso database con un secondo
  scheduler. Una seconda copia avviata all'accesso non apre la finestra.
- Se l'area di notifica non è disponibile (su Linux manca
  `libayatana-appindicator`), attivare la modalità mostra l'errore e la
  modalità resta spenta: senza icona la finestra nascosta si potrebbe
  riaprire solo rilanciando l'app. I pacchetti `.deb`/`.rpm` dichiarano la
  libreria come dipendenza.

## Alternative scartate

- **Servizio di sistema o timer separato** (systemd user timer, Utilità di
  pianificazione di Windows) che legge il database: un secondo programma
  con accesso ai dati, da installare e aggiornare a parte, e notifiche
  senza le azioni dell'app.
- **Plugin autostart di Tauri**: su Linux usa l'eseguibile corrente, che per
  un'AppImage è un percorso temporaneo; la soluzione diretta è breve e
  gestisce l'AppImage.

## Limiti

- Su GNOME senza estensione per l'area di notifica l'icona non si vede: la
  finestra si riapre aprendo di nuovo Greatnote dal menu delle app.
- Con il computer spento o sospeso i promemoria arrivano alla riaccensione,
  come «persi».
