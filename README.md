# Greatnote

Applicazione desktop di note local-first, inizialmente per Linux e predisposta
per Windows. La UI è costruita con React e CSS standard in Tauri: non usa API
specifiche di KDE e lascia decorazioni e integrazione della finestra al window
manager corrente.

## Stato

Progetto in sviluppo. Note, spazi, taccuini, tag, bozze, cestino, ricerca
full-text, preferite, archivio, recenti, allegati locali e collegamenti tra
note sono persistiti in locale. Sono disponibili importazione (Markdown,
testo, HTML sanificato, archivio Greatnote) con anteprima, esportazione
(Markdown, HTML, testo, PDF tramite stampa, ZIP, archivio completo), backup
manuali e automatici verificabili, copie di backup cifrate con password,
ripristino e controllo d'integrità. Attività, promemoria con notifiche di
sistema e calendario locale sono disponibili; i promemoria scattano mentre
l'app è aperta (quelli persi vengono mostrati al successivo avvio).
Le funzioni AI (assistente con fonti citate, modifiche con anteprima, tag e
attività suggeriti, ricerca per significato) sono facoltative, disattivate
per impostazione predefinita e funzionano con un server locale compatibile
OpenAI come Ollama; un server remoto richiede consenso esplicito.
Un server MCP locale in sola lettura (`greatnote --mcp`, disattivato per
impostazione predefinita) permette ad app come Claude Code di cercare e
leggere le note: si attiva in Impostazioni › Integrazioni. Nella stessa
sezione si possono aggiungere calendari iCal (Google Calendar, Outlook…) in
sola lettura incollandone l'indirizzo, senza login.
La sincronizzazione tra dispositivi usa l'account Dropbox (accesso nel
browser) oppure una cartella già sincronizzata (Nextcloud, Syncthing…),
senza server né account Greatnote. Non è necessario un account né
una connessione.

## Prerequisiti

- Rust stable e Cargo.
- Node.js 22.12 o successivo (oppure 20.19+) e npm.
- Per avviare la finestra su Linux, dipendenze native Tauri 2 (GTK 3,
  WebKitGTK 4.1 e dipendenze correlate). Consultare la guida ufficiale Tauri
  per i nomi aggiornati dei pacchetti Fedora.
- Per Windows, dipendenze Tauri 2 e WebView2 secondo la guida ufficiale.

Questa sessione è stata esaminata su Fedora 44 KDE con Rust 1.98.1, Node
22.23.1 e npm 10.9.8. Le dipendenze Tauri risultano installate e
`cargo check -p greatnote-desktop` compila correttamente. L'utente ha avviato
`npm run tauri:dev`; `rustfmt` e Clippy sono installati dai pacchetti Fedora
(`sudo dnf install rustfmt clippy`).

Su Fedora, installare le dipendenze native prima dell'avvio desktop:

```sh
sudo dnf install gtk3-devel webkit2gtk4.1-devel javascriptcoregtk4.1-devel \
  libappindicator-gtk3-devel libsoup3-devel librsvg2-devel \
  openssl-devel curl wget file
```

`libsoup3-devel` fornisce il file `libsoup-3.0.pc` richiesto da `pkg-config`
e `javascriptcoregtk4.1-devel` fornisce `javascriptcoregtk-4.1.pc`;
`webkit2gtk4.1-devel` fornisce gli altri header e file `.pc` per WebKitGTK.
Dopo aver installato i pacchetti, riprova `npm run tauri:dev`; normalmente non
serve impostare `PKG_CONFIG_PATH`, perché Fedora registra i file `.pc` nei
percorsi standard.

Il logo originale (1024×1024, sfondo trasparente) è
`docs/branding/logo.png`. Da questo sono ricavati, ritagliati sul
contenuto e ridimensionati, `src-tauri/icons/icon.png` (512×512, icona della
finestra e dei bundle Linux) e `frontend/src/assets/logo.png` (128×128, logo
nella barra laterale).

Su Wayland (KDE, GNOME) l'icona e il nome nella barra delle applicazioni
vengono dal file `.desktop` con lo stesso app_id della finestra,
`dev.greatnote.notes` (opzione `enableGTKAppId`). I pacchetti .deb/.rpm lo
installano con il modello `src-tauri/linux/greatnote.desktop`; per la build di
sviluppo eseguire una volta:

```sh
./scripts/install-dev-desktop-entry.sh          # installa voce e icona
./scripts/install-dev-desktop-entry.sh --remove # le rimuove
```

Con l'app_id GTK attivo può essere aperta una sola istanza dell'app alla
volta. La scritta usa Manrope (SIL OFL 1.1, pacchetto
`@fontsource/manrope`): "Great" in Bold, "note" in SemiBold.

## Installazione e avvio

```sh
npm ci
npm run tauri:dev
```

Per includere il collegamento a Google Calendar e Outlook (solo accesso e
consenso per l'utente) servono le registrazioni dell'app come editore:
copia `oauth.env.example` in `oauth.env` (escluso da git), compila gli ID
come spiegato in [docs/OAUTH_SETUP.md](docs/OAUTH_SETUP.md) e avvia con
`set -a; source oauth.env; set +a; npm run tauri:dev`.

## Test e build

```sh
# Core Rust, senza necessità delle librerie grafiche native
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# Frontend
npm run build

# Interfaccia: Firefox headless con backend simulato (richiede Firefox)
npm run test:ui
# Schermate di revisione grafica, tema chiaro e scuro (target/ui-screens/)
npm run ui:screens

# App desktop e bundle
npm run tauri:build
```

## Installer per Windows

La configurazione Windows è in `src-tauri/tauri.windows.conf.json`, che Tauri
unisce a `tauri.conf.json` solo quando compila su Windows. Produce due
installer x64:

- `target/release/bundle/nsis/Greatnote_<versione>_x64-setup.exe`: quello
  consigliato per i colleghi. Si installa per l'utente corrente, senza
  permessi di amministratore, in italiano o inglese.
- `target/release/bundle/msi/Greatnote_<versione>_x64_en-US.msi` e
  `..._it-IT.msi`: per la distribuzione centralizzata (GPO, Intune).

WebView2 è già presente su Windows 10/11 aggiornati. Se manca, l'installer
lo scarica e lo installa in modo silenzioso, quindi in quel caso serve la
rete. I dati restano in `%APPDATA%\dev.greatnote.notes`, e la
disinstallazione non li cancella.

**Compilare su GitHub Actions (consigliato).** Il workflow
`.github/workflows/windows-installer.yml` compila su un runner Windows pulito,
esegue i test e pubblica gli installer come artefatto `greatnote-windows-x64`.
Si avvia a mano da *Actions › Windows installer › Run workflow*, oppure con
un tag:

```sh
git tag v0.1.0 && git push origin v0.1.0
```

I collegamenti a Google, Outlook e Dropbox funzionano solo se si definiscono i
segreti del repository `GREATNOTE_GOOGLE_CLIENT_ID`,
`GREATNOTE_GOOGLE_CLIENT_SECRET`, `GREATNOTE_MICROSOFT_CLIENT_ID` e
`GREATNOTE_DROPBOX_APP_KEY` (vedi [docs/OAUTH_SETUP.md](docs/OAUTH_SETUP.md)).
Senza di essi l'app indica quelle funzioni come non configurate.

**Compilare in locale.** Servono Rust stable con target MSVC
(`winget install Rustlang.Rustup`), Visual Studio Build Tools con il
carico di lavoro "Sviluppo di applicazioni desktop con C++" e Node.js:

```powershell
npm ci
npm run tauri:build
```

Con **Smart App Control** attivo (Sicurezza di Windows › Controllo app e
browser) la compilazione fallisce con `os error 4551`. Il motivo è che
Windows blocca gli script di build appena compilati da Cargo, perché non
sono firmati. In quel caso conviene usare GitHub Actions.

**Firma del codice.** Gli installer non sono firmati, quindi alla prima
apertura SmartScreen mostra «Windows ha protetto il PC» (*Ulteriori
informazioni › Esegui comunque*). Sui PC con Smart App Control attivo
l'installazione può essere bloccata del tutto. Per una distribuzione
estesa serve un certificato di firma del codice (vedi ADR 0019).

## Risoluzione problemi

Se la finestra di sviluppo appare vuota, controlla prima gli errori di runtime
nel terminale di `npm run tauri:dev`. L'editor ignora le istanze Tiptap già
distrutte durante il ciclo di vita React. La CSP di sviluppo consente il
preambolo React Refresh inline di Vite e il relativo WebSocket HMR locale;
la build mantiene una CSP distinta e più restrittiva. Errori `Port 1420 is
already in use` indicano invece un server Vite già attivo.

## Dati locali

Il database SQLite `gna.sqlite3` è creato nella directory dati dell'app
restituita da Tauri, seguendo le convenzioni della piattaforma (XDG su Linux).
Gli allegati sono file separati nella sottocartella `attachments/`. I backup
sono archivi ZIP nella sottocartella `backups/`; un backup viene creato
automaticamente prima di ogni aggiornamento dello schema. Usa *Dati e backup*
nella barra laterale per creare, verificare, copiare altrove e ripristinare i
backup, anche in copia cifrata con password (formato aperto age). Disinstallare
l'app non cancella questa cartella.

Su Linux la cartella dati è normalmente `~/.local/share/dev.greatnote.notes/`.
Al primo avvio dopo il cambio di nome, database, allegati e backup di
`~/.local/share/dev.gna.notes/` vengono spostati automaticamente (le sole cache
del motore web restano lì e la cartella si può poi eliminare); il nome del
file `gna.sqlite3` e i formati `gna-archive`/`gna-backup` restano invariati
per compatibilità (ADR 0006).

## Ricerca e produttività

La ricerca è locale e usa FTS5. Esempi di filtri: `tag:progetto`,
`notebook:personale`, `space:lavoro`, `created:2026-01-01..2026-12-31`,
`updated:7d`, `has:attachment`, `is:pinned`, `is:archived`, `is:task`,
`before:2026-10-01` e `after:2026-01-01`. Le frasi tra virgolette sono
supportate. Gli operatori booleani e le parentesi non sono ancora supportati:
inserirli come operatori espliciti mostra un errore, senza eseguire query
parziali.

Gli allegati supportati sono file di testo (anche Markdown/CSV), PNG, JPEG,
GIF e PDF fino a 25 MiB ciascuno. Il tipo è rilevato dai contenuti e
confrontato con l'estensione; gli allegati di testo vengono indicizzati fino
ai primi 100.000 caratteri. I collegamenti tra note sono locali e non
includono note cestinate o archiviate.

## Importare da altre app

**File › Importa…** importa file ENEX di Evernote, archivi JEX di Joplin e
vault di Obsidian mantenendo taccuini, tag, date, allegati e collegamenti tra
note, con anteprima prima di confermare. Dettagli nella
[guida utente](./docs/USER_GUIDE.md#importare-da-evernote-joplin-e-obsidian)
e in [ADR 0011](./docs/decisions/0011-import-from-evernote-joplin-obsidian.md).

## Funzioni AI (facoltative)

Tutto è spento finché non lo attivi in **Impostazioni › Intelligenza
artificiale**.

Il modo più semplice è il provider **Integrato**: l'app propone i modelli
da scaricare per chat, contesto e visione e li esegue con llama.cpp, che è
incluso nell'installer Windows. Su Linux serve `llama-server` nel `PATH`.
Per una build Windows locale, prima di `npx tauri build` scarica il motore
con:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/fetch-llama.ps1
```

Oltre ai modelli consigliati puoi cercarne altri su Hugging Face. Ogni
variante mostra dimensione, memoria stimata e se è adatta al computer
(scheda video, processore, al limite, troppo grande).

Dettagli in [ADR 0020](./docs/decisions/0020-builtin-ai-engine.md) e
[ADR 0021](./docs/decisions/0021-hugging-face-model-search.md).

In alternativa, per usare un modello locale con [Ollama](https://ollama.com):

```sh
ollama pull qwen3:8b            # modello per chat e scrittura (esempio)
ollama pull nomic-embed-text    # facoltativo, per la ricerca per significato
```

Poi nelle impostazioni scegli il preset Ollama
(`http://localhost:11434/v1`), premi **Prova la connessione**, indica i
modelli e salva. Funziona con qualunque server compatibile con l'API OpenAI
(Open WebUI con `http://localhost:3000/api` e una sua chiave API, LM Studio,
oppure **Claude Code** installato sul computer, con il tuo account,
llama.cpp server con `--embeddings` per gli embedding, servizi
remoti via `https` con chiave salvata nel portachiavi di sistema). Con un
server esterno Greatnote non scarica modelli. Dettagli in [ADR 0010](./docs/decisions/0010-optional-ai-providers-and-semantic-search.md)
e nella [guida utente](./docs/USER_GUIDE.md#assistente-ai-facoltativo).

## Documentazione

- [Architettura](./ARCHITECTURE.md)
- [Roadmap](./ROADMAP.md)
- [Guida utente](./docs/USER_GUIDE.md)
- [Sicurezza](./docs/SECURITY.md)
- [Test](./docs/TESTING.md)
- [Formati di esportazione e backup](./docs/ARCHIVE_FORMAT.md)
- [Decisioni architetturali](./docs/decisions/)
- [Specifiche operative](./CLAUDE.md)

## Licenza

Greatnote è software libero distribuito con licenza
[GNU General Public License v3.0](./LICENSE) (`GPL-3.0-only`).
