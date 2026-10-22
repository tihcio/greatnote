# Checklist di rilascio

Da seguire per ogni versione pubblicata (anche di prova). Una voce non
verificata va riportata nelle note di rilascio come limite noto, non
saltata in silenzio.

## 1. Controlli automatici

```bash
scripts/release-check.sh        # aggiungi --ui per il test dell'interfaccia (Firefox)
```

Lo script si ferma al primo errore ed esegue:

- `cargo fmt --all --check`;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
- `cargo test --workspace` (include i test di backup, ripristino,
  migrazioni da schemi precedenti e sincronizzazione);
- `cargo audit` (RustSec, richiede `cargo install cargo-audit --locked`);
- `npm ci`, `npm run build`, `npm audit`;
- ricerca di chiavi API o segreti OAuth nei file versionati.

La CI (`.github/workflows/ci.yml`) ripete gli stessi controlli su Linux e
Windows, più il job `rust-audit`. Il tag di versione non va creato se la CI
sul commit non è verde.

**Avvisi di dipendenze accettati** (non vulnerabilità, nessuna correzione
disponibile da questo progetto): `glib`, `proc-macro-error` (GTK 3 usato
da Tauri su Linux), `zbus_names`, `zvariant`, `zvariant_derive`
(notifiche D-Bus), `basic-toml` (macro di localizzazione di `age`, solo in
compilazione). Ricontrollarli a ogni rilascio: se compare una
**vulnerabilità**, `cargo audit` fallisce e il rilascio si blocca finché
la dipendenza non è aggiornata (`cargo update -p <crate>`).

## 2. Versione e documenti

- [ ] Stessa versione in `Cargo.toml`, `src-tauri/Cargo.toml`,
      `src-tauri/tauri.conf.json` e `package.json`.
- [ ] `CHANGELOG.md`: la sezione "Non rilasciato" diventa la versione con
      data; limiti noti elencati.
- [ ] `ROADMAP.md`, `README.md` e `docs/USER_GUIDE.md` descrivono solo
      funzioni realmente disponibili; le integrazioni che richiedono
      configurazione (OAuth, AI remota) sono indicate come tali.
- [ ] Se lo schema del database è cambiato: nuova migrazione numerata,
      `CURRENT_SCHEMA_VERSION` aggiornato e test di migrazione da un
      database esistente.
- [ ] Se il formato dei backup o dell'archivio è cambiato:
      `docs/ARCHIVE_FORMAT.md` aggiornato e lettura dei formati precedenti
      ancora coperta da test.

## 3. Prova di backup e ripristino (manuale, in un profilo di prova)

Su Linux: `XDG_DATA_HOME=$(mktemp -d) npm run tauri:dev`. Su Windows usa
un account utente di prova o una macchina virtuale.

- [ ] Crea note con tag, allegato e immagine; crea un backup manuale e
      premi **Verifica**.
- [ ] Modifica una nota, **Ripristina** il backup: la finestra mostra i
      dati del backup. Ripristina poi il backup "Prima del ripristino".
- [ ] **Copia cifrata…** del backup, poi **Ripristina da file…** con
      password errata (errore) e giusta (ripristino). Nessun file
      `.decrypted-*` resta nella cartella dati.
- [ ] Installa la nuova versione sopra la precedente con dati esistenti:
      compare il backup `greatnote-premigration-…` se lo schema è
      cambiato, e le note sono intatte.
- [ ] **Integrità**: nessun problema segnalato.

## 4. Pacchetti

### Linux

- [ ] `npx tauri build` produce `.deb`, `.rpm` e AppImage con
      `llama-server` incluso (`resources/llama`).
- [ ] Nuova versione in `<releases>` di
      `src-tauri/linux/dev.greatnote.notes.metainfo.xml` (lo controlla un
      test) e `appstreamcli validate --no-net` su quel file.
- [ ] Nella release i pacchetti Linux si chiamano `Greatnote.x86_64.rpm`,
      `Greatnote.amd64.deb` e `Greatnote.x86_64.AppImage`; aprendo il
      `.rpm` in Discover compare «Greatnote», e dopo l'installazione l'app
      ha icona e descrizione tra le installate.
- [ ] Installazione del `.deb` su una distribuzione Debian/Ubuntu e avvio
      dal menu; icona visibile anche su Wayland
      (`StartupWMClass=dev.greatnote.notes`).
- [ ] Dati in `~/.local/share/dev.greatnote.notes` (o `$XDG_DATA_HOME`).
- [ ] Disinstallazione: la cartella dati resta.

### Windows

- [ ] Workflow *Installers* verde; scaricati `.exe` (NSIS) e `.msi`.
- [ ] Su un PC o una VM puliti: installazione senza permessi di
      amministratore (NSIS), avvio, notifica di un promemoria, credenziale
      salvata in Gestione credenziali, apertura di un allegato.
- [ ] Cartella dati e nomi file con spazi e caratteri accentati.
- [ ] Aggiornamento sopra la versione precedente e disinstallazione senza
      perdita della cartella dati.
- [ ] Gli installer non sono firmati: SmartScreen mostra un avviso.
      Indicarlo nelle note di rilascio finché non c'è un certificato.

## 5. Sicurezza

- [ ] Nessuna nuova capability Tauri o permesso di plugin esposto al
      frontend senza motivazione (`src-tauri/capabilities/default.json`).
- [ ] CSP della build invariata o più restrittiva
      (`src-tauri/tauri.conf.json`); nessuna origine remota.
- [ ] Nuovi comandi Tauri convalidano l'input lato Rust.
- [ ] I log di una sessione di prova non contengono titoli, contenuti,
      percorsi personali o token.
- [ ] Nessun segreto nei file di build: gli ID OAuth arrivano solo dai
      segreti del repository (`oauth.env.example` contiene solo segnaposto).

## 6. Pubblicazione

- [ ] Tag `vX.Y.Z` sul commit verificato; il workflow *Installers*
      parte dal tag, firma gli installer e crea la release in bozza con
      `latest.json` ([UPDATES.md](./UPDATES.md)).
- [ ] Nella bozza: `latest.json` indica la versione giusta e l'URL
      dell'installer della stessa release; ci sono anche `.deb`, `.rpm` e
      AppImage del job Linux.
- [ ] Prova di aggiornamento: su un PC con la versione precedente
      installata, dopo la pubblicazione **Cerca aggiornamenti** trova la
      nuova versione, **Riavvia e aggiorna** la installa e le note restano.
- [ ] Note di rilascio con: novità, limiti noti, avvisi di dipendenze
      accettati, stato della firma degli installer e procedura di
      ripristino.

## Ritorno alla versione precedente

1. Disinstalla la nuova versione: la cartella dati non viene cancellata.
2. Se lo schema era cambiato, la versione precedente rifiuta il database
   più recente. Installa la versione precedente in un profilo pulito e
   usa **Ripristina da file…** sul backup `greatnote-premigration-…` che
   la nuova versione ha creato prima della migrazione (le modifiche fatte
   dopo l'aggiornamento non sono in quel backup: esportale prima, per
   esempio in Markdown).
3. Con la sincronizzazione attiva vedi la nota sul ripristino in
   `docs/USER_GUIDE.md`: il computer riceve una nuova identità e si
   ricongiunge senza perdere note.
