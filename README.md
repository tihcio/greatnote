# Greatnote

Greatnote è un'app desktop per prendere e organizzare note, con un
approccio **local-first**: i dati vengono salvati sul computer e non
serve creare un account né essere connessi a Internet per usare le
funzioni principali.

È sviluppata con **React**, **Rust** e **Tauri 2**. Il progetto è in
sviluppo e nasce per Linux, con supporto anche per Windows.

## Funzionalità

-   **Organizzazione delle note:** spazi, taccuini, tag, preferiti,
    archivio, bozze, cestino e note recenti.
-   **Ricerca full-text locale:** ricerca rapida nel contenuto delle
    note, con filtri per tag, taccuino, spazio, date, allegati e tipo di
    contenuto.
-   **Collegamenti tra note:** crea riferimenti interni per collegare
    informazioni correlate.
-   **Attività e promemoria:** gestisci attività, promemoria con
    notifiche di sistema e un calendario locale.
-   **Allegati:** aggiungi file di testo, Markdown, CSV, immagini e PDF
    alle note.
-   **Importazione ed esportazione:** importa note da Evernote, Joplin e
    Obsidian; esporta in diversi formati.
-   **Backup e ripristino:** crea backup manuali o automatici, verifica
    l'integrità e proteggi le copie con una password.
-   **Sincronizzazione tra dispositivi:** usa Dropbox oppure una
    cartella sincronizzata tramite strumenti come Nextcloud o Syncthing.
-   **Funzioni AI facoltative:** assistente con fonti citate, proposte
    di modifica con anteprima, suggerimenti per tag e attività e ricerca
    semantica.
-   **Integrazioni:** consulta calendari iCal in sola lettura e consenti
    a strumenti compatibili di cercare e leggere le note tramite un
    server MCP locale in sola lettura.

Le funzioni AI sono disattivate per impostazione predefinita. Puoi usare
modelli locali tramite il motore integrato o un servizio compatibile con
l'API OpenAI. L'utilizzo di un server remoto richiede un consenso
esplicito.

## Stato del progetto

Greatnote è in sviluppo. Alcune funzionalità e integrazioni possono
richiedere una configurazione aggiuntiva. Prima di affidare dati
importanti all'app, è consigliabile verificare il funzionamento dei
backup e conservarne una copia separata.

## Installazione e avvio in modalità sviluppo

### Prerequisiti

-   [Rust stable](https://www.rust-lang.org/tools/install) e Cargo
-   [Node.js](https://nodejs.org/) 22.12 o successivo, oppure 20.19 o
    successivo
-   npm
-   Su Linux, le dipendenze native richieste da Tauri 2, tra cui GTK 3 e
    WebKitGTK 4.1
-   Su Windows, gli strumenti di compilazione richiesti da Tauri 2 e
    WebView2

### Fedora

Installa le dipendenze native:

``` sh
sudo dnf install gtk3-devel webkit2gtk4.1-devel javascriptcoregtk4.1-devel \
  libappindicator-gtk3-devel libsoup3-devel librsvg2-devel \
  openssl-devel curl wget file
```

### Avviare l'app

Dalla directory del progetto:

``` sh
npm ci
npm run tauri:dev
```

Per configurare i collegamenti con Google Calendar e Outlook in
sviluppo, consulta [docs/OAUTH_SETUP.md](docs/OAUTH_SETUP.md). Queste
integrazioni richiedono la configurazione delle credenziali
dell'applicazione.

## Compilazione

Per compilare l'app desktop sulla piattaforma corrente:

``` sh
npm run tauri:build
```

### Windows

Il progetto include una configurazione Tauri specifica per Windows. La
build produce installer x64 in formato **NSIS** e **MSI**:

-   **NSIS**: installazione per l'utente corrente, senza privilegi di
    amministratore.
-   **MSI**: adatto alla distribuzione centralizzata, ad esempio tramite
    GPO o Intune.

WebView2 è normalmente presente sulle versioni aggiornate di Windows 10
e Windows 11. Se non è disponibile, l'installer può scaricarlo e
installarlo; in questo caso è necessaria una connessione di rete.

Gli installer generati dal progetto non sono firmati digitalmente.
Windows può quindi mostrare un avviso SmartScreen e, con Smart App
Control attivo, può bloccare l'installazione o la compilazione locale.

#### Build tramite GitHub Actions

Il workflow `.github/workflows/windows-installer.yml` consente di
compilare l'app su un runner Windows e pubblicare gli installer come
artefatto `greatnote-windows-x64`.

Puoi avviarlo da **Actions → Windows installer → Run workflow**, oppure
creare e pubblicare un tag Git, ad esempio:

``` sh
git tag v0.1.0
git push origin v0.1.0
```

Per abilitare i collegamenti a Google Calendar, Outlook e Dropbox nella
build, configura i segreti del repository descritti in
[docs/OAUTH_SETUP.md](docs/OAUTH_SETUP.md).

Con un tag `v*` il workflow firma gli installer per gli **aggiornamenti
in-app** e crea una release in bozza con `latest.json`: pubblicandola, le
app installate la scaricano e la installano da sole. Configurazione
(repository pubblico, segreti della chiave di firma) e procedura in
[docs/UPDATES.md](docs/UPDATES.md).

#### Build locale su Windows

Sono necessari Rust con target MSVC, Visual Studio Build Tools con il
carico di lavoro **Sviluppo di applicazioni desktop con C++** e Node.js.

``` powershell
npm ci
npm run tauri:build
```

Per includere il motore AI integrato nella build Windows, esegui prima:

``` powershell
powershell -ExecutionPolicy Bypass -File scripts/fetch-llama.ps1
```

## Test

I principali controlli automatici possono essere eseguiti con questi
comandi:

``` sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
npm run build
```

I test dell'interfaccia richiedono Firefox:

``` sh
npm run test:ui
```

Per generare le schermate dell'interfaccia nei temi chiaro e scuro:

``` sh
npm run ui:screens
```

## Ricerca

La ricerca utilizza SQLite FTS5 ed è eseguita localmente. Sono
disponibili filtri come:

  Filtro                    Esempio
  ------------------------- ----------------------------------
  Tag                       `tag:progetto`
  Taccuino                  `notebook:personale`
  Spazio                    `space:lavoro`
  Intervallo di creazione   `created:2026-01-01..2026-12-31`
  Aggiornamento recente     `updated:7d`
  Con allegati              `has:attachment`
  Preferite                 `is:pinned`
  Archiviate                `is:archived`
  Attività                  `is:task`
  Prima di una data         `before:2026-10-01`
  Dopo una data             `after:2026-01-01`

Le frasi tra virgolette sono supportate. Gli operatori booleani e le
parentesi non sono attualmente disponibili.

## Importazione ed esportazione

Da **File → Importa...** puoi importare:

-   file ENEX di Evernote;
-   archivi JEX di Joplin;
-   vault di Obsidian.

L'importazione offre un'anteprima prima della conferma e conserva, dove
supportato, taccuini, tag, date, allegati e collegamenti tra note.

Sono inoltre disponibili l'importazione di Markdown, testo, HTML
sanificato e archivi Greatnote, e l'esportazione in Markdown, HTML,
testo, PDF tramite stampa, ZIP e archivio completo.

Per i dettagli consulta la [guida
utente](docs/USER_GUIDE.md#importare-da-evernote-joplin-e-obsidian).

## Intelligenza artificiale

Le funzioni AI si attivano da **Impostazioni → Intelligenza
artificiale**. Sono opzionali e possono essere usate con il motore
integrato basato su `llama.cpp`, con [Ollama](https://ollama.com) o con
un server compatibile con l'API OpenAI.

Per esempio, con Ollama puoi scaricare un modello per chat e scrittura
e, facoltativamente, un modello per gli embedding:

``` sh
ollama pull qwen3:8b
ollama pull nomic-embed-text
```

Nelle impostazioni seleziona il provider, verifica la connessione e
indica i modelli da usare. Con un server esterno Greatnote non scarica i
modelli. Sono supportati anche altri server compatibili, tra cui Open
WebUI e LM Studio.

Per ulteriori dettagli consulta la [guida
utente](docs/USER_GUIDE.md#assistente-ai-facoltativo) e le note tecniche
[ADR
0010](docs/decisions/0010-optional-ai-providers-and-semantic-search.md),
[ADR 0020](docs/decisions/0020-builtin-ai-engine.md) e [ADR
0021](docs/decisions/0021-hugging-face-model-search.md).

## Dati e privacy

Il database delle note è SQLite e viene conservato nella directory dati
dell'app. Gli allegati sono archiviati come file separati; i backup si
trovano nella relativa sottocartella. Greatnote crea inoltre un backup
prima degli aggiornamenti dello schema del database.

Su Linux, la directory dati predefinita è normalmente:

``` text
~/.local/share/dev.greatnote.notes/
```

Su Windows i dati sono conservati in:

``` text
%APPDATA%\\dev.greatnote.notes
```

La disinstallazione non elimina la cartella dei dati. Puoi gestire,
verificare, esportare e ripristinare i backup dalla sezione **Dati e
backup** dell'app.

Le funzioni principali non richiedono un account Greatnote. La
sincronizzazione Dropbox richiede l'accesso al proprio account Dropbox;
in alternativa si può usare una cartella gestita da un servizio di
sincronizzazione esterno. Le funzioni AI possono essere configurate per
usare modelli locali; l'uso di servizi remoti richiede consenso
esplicito.

Per informazioni più dettagliate, consulta
[docs/SECURITY.md](docs/SECURITY.md).

## Documentazione

-   [Guida utente](docs/USER_GUIDE.md)
-   [Architettura](ARCHITECTURE.md)
-   [Roadmap](ROADMAP.md)
-   [Sicurezza](docs/SECURITY.md)
-   [Test](docs/TESTING.md)
-   [Checklist di rilascio](docs/RELEASE_CHECKLIST.md)
-   [Aggiornamenti in-app e pubblicazione](docs/UPDATES.md)
-   [Formati di esportazione e backup](docs/ARCHIVE_FORMAT.md)
-   [Decisioni architetturali](docs/decisions/)

## Licenza

Greatnote è software libero distribuito secondo i termini della [GNU
General Public License v3.0](LICENSE) (`GPL-3.0-only`).
