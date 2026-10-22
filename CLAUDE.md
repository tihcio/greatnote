# Claude Code — Specifica di progetto: applicazione note desktop in Rust

> **Nome provvisorio:** Greatnote
> **Piattaforma iniziale:** Linux desktop  
> **Portabilità futura:** Windows  
> **Lingua del prodotto:** italiano e inglese, con struttura pronta per altre lingue  
> **Obiettivo:** costruire da zero un'applicazione di produttività e knowledge management ispirata alle categorie funzionali di Evernote, con codice originale, architettura modulare, dati sotto il controllo dell'utente e funzionalità AI opzionali.

---

## 0. Istruzioni fondamentali per Claude Code

Agisci come **tech lead, architetto software, sviluppatore Rust senior, UX designer e QA engineer**. Non limitarti a produrre esempi o una demo: realizza un prodotto funzionante, testabile, documentato e progressivamente completo.

### Regole di lavoro

1. **Esamina prima l'ambiente**: controlla repository, file esistenti, toolchain Rust, dipendenze e sistema operativo. Non sovrascrivere lavoro preesistente senza capirlo.
2. Se il repository è vuoto, inizializzalo. Se contiene già codice, integrati nella struttura esistente e spiega le decisioni importanti.
3. Crea e mantieni aggiornati:
   - `README.md`
   - `ARCHITECTURE.md`
   - `ROADMAP.md`
   - `CHANGELOG.md`
   - `docs/decisions/` con ADR per decisioni architetturali importanti
   - `docs/USER_GUIDE.md`
   - `docs/SECURITY.md`
   - `docs/TESTING.md`
   - `CLAUDE.md` con le istruzioni operative specifiche del repository
4. Prima di implementare una fase, scrivi o aggiorna il piano concreto, poi implementa e verifica. Non fermarti al piano se puoi procedere autonomamente.
5. Lavora in incrementi verticali: una funzionalità deve includere UI, logica, persistenza, gestione degli errori e test pertinenti.
6. Esegui `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` e `cargo test` quando applicabili. Risolvi gli errori introdotti prima di proseguire.
7. Non dichiarare una funzione completata se è soltanto un mock, una schermata statica o un pulsante senza comportamento reale. Se una funzione è temporaneamente simulata, etichettala chiaramente nella UI e nella documentazione.
8. Evita `unwrap()`/`expect()` nei percorsi di produzione, salvo invarianti motivate. Usa errori tipizzati, log strutturati e messaggi comprensibili.
9. Non aggiungere dipendenze senza motivo: verifica manutenzione, licenza, compatibilità Linux/Windows e implicazioni di sicurezza.
10. Non inserire chiavi API, token, credenziali o dati personali nel codice, nei log o nel repository.
11. Non copiare codice, branding, icone, testi proprietari o asset di Evernote. Implementa funzionalità analoghe con identità visiva originale. Non promettere interoperabilità con servizi proprietari senza verificarne API e condizioni d'uso.
12. Se mancano informazioni non bloccanti, scegli un default ragionevole e documentalo. Chiedi all'utente soltanto per decisioni irreversibili, costi esterni, accessi o requisiti che non puoi decidere in sicurezza.
13. Mantieni l'app avviabile durante lo sviluppo. Dopo ogni fase, riporta modifiche, test eseguiti, limitazioni e prossimo passo.
14. Non tentare di implementare l'intero prodotto in un'unica modifica gigantesca. Procedi per fasi, ma continua fino a ottenere il massimo risultato verificabile nel contesto disponibile.

### Criterio di completamento

Una feature è “completa” solo se:
- il flusso utente è utilizzabile nella UI;
- i dati sono persistiti e sopravvivono a riavvio;
- gli errori sono gestiti;
- esistono test adeguati;
- è documentata;
- non è presentata come funzionante se dipende da servizi non configurati.

---

## 1. Visione del prodotto

Realizza un'app desktop per note, appunti, ricerca e gestione della conoscenza. Deve offrire un'esperienza rapida e affidabile, con:

- note ricche e modificabili;
- taccuini, spazi, tag, collegamenti e allegati;
- ricerca full-text e filtri avanzati;
- attività, scadenze e promemoria;
- funzionamento offline come modalità predefinita;
- sincronizzazione opzionale tra dispositivi, predisposta architetturalmente ma non simulata;
- esportazione e importazione in formati aperti;
- funzioni AI con provider configurabili, inclusi modelli locali;
- interfaccia per estensioni e integrazioni;
- protezione dei dati e possibilità di backup;
- predisposizione a una futura versione Windows senza riscrivere il dominio applicativo.

L'app deve funzionare in modalità locale anche senza account, rete, cloud o chiavi API. Le funzioni online devono essere aggiuntive, non necessarie per leggere e modificare le note locali.

### Principi

1. **Local-first:** il database locale è la fonte di verità per l'uso offline.
2. **Privacy by design:** minimizzazione dei dati, consenso esplicito per invii a servizi remoti.
3. **Open formats:** esportazione documentata senza lock-in.
4. **Accessibilità:** navigazione da tastiera, contrasto, focus visibile e supporto screen reader ove offerto dal toolkit.
5. **Affidabilità:** migrazioni del database, backup verificabili, recupero dagli errori.
6. **Portabilità:** logica di dominio indipendente dalla GUI e dal sistema operativo.
7. **Trasparenza:** indicare chiaramente stato di sincronizzazione, AI, backup e integrazioni.

---

## 2. Stack tecnico richiesto

Scegli versioni stabili e compatibili al momento dell'implementazione; blocca le versioni nel lockfile e documenta i requisiti minimi.

### Desktop

- **Rust stable** per logica applicativa, dominio, storage e servizi.
- **Tauri 2** come shell desktop e bridge sicuro verso la UI.
- **Frontend TypeScript** con React e Vite, oppure Svelte se motivato. Preferenza iniziale: React + TypeScript.
- CSS con design system leggero; evitare dipendenze UI pesanti senza necessità.
- Per Windows, mantenere i comandi Tauri e i servizi Rust indipendenti dal sistema operativo.

Motivazione: Tauri consente di avere il nucleo applicativo in Rust e un'interfaccia desktop multipiattaforma. Se una limitazione reale impedisce una funzione necessaria, documentare il problema e valutare un'alternativa prima di cambiare stack.

### Persistenza locale

- SQLite tramite `sqlx` o una libreria Rust equivalente ben mantenuta.
- Migrazioni SQL versionate e applicate in modo atomico.
- FTS5 per la ricerca full-text, verificando la disponibilità nella build SQLite scelta.
- Transazioni per operazioni che coinvolgono più entità.
- Database versionato e strategia di backup prima delle migrazioni.
- File binari e allegati su filesystem, con metadati nel database. Non inserire grandi file direttamente nelle righe SQL senza una motivazione.

### Componenti indicativi

Usa queste librerie solo dopo averne verificato compatibilità, licenza e manutenzione:

- serializzazione: `serde`;
- errori: `thiserror` per errori di dominio, `anyhow` ai confini applicativi appropriati;
- log: `tracing` e `tracing-subscriber`;
- identificatori: UUID;
- date e fusi orari: `time` o `chrono`, con gestione esplicita del fuso;
- ricerca: SQLite FTS5; in seguito indice vettoriale locale separato;
- sicurezza password/token: keyring del sistema operativo quando disponibile, mai testo in chiaro;
- test: unitari, integrazione, test UI e test end-to-end secondo la maturità del progetto.

Non assumere che una specifica libreria sia già installata o adatta: verifica le API aggiornate e compila frequentemente.

---

## 3. Architettura

Organizza il codice in moduli o crate con responsabilità chiare. Evita un singolo file gigante e impedisci alla UI di accedere direttamente al database.

Struttura indicativa:

```text
/
├── CLAUDE.md
├── README.md
├── ARCHITECTURE.md
├── ROADMAP.md
├── CHANGELOG.md
├── Cargo.toml                 # se workspace Rust
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/
│   ├── migrations/
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── commands/
│       ├── domain/
│       ├── application/
│       ├── storage/
│       ├── search/
│       ├── sync/
│       ├── ai/
│       ├── integrations/
│       ├── security/
│       ├── reminders/
│       ├── import_export/
│       └── platform/
├── frontend/
│   ├── src/
│   │   ├── app/
│   │   ├── components/
│   │   ├── features/
│   │   ├── hooks/
│   │   ├── lib/
│   │   ├── stores/
│   │   ├── styles/
│   │   └── types/
│   ├── package.json
│   └── ...
├── tests/
├── docs/
└── scripts/
```

Adatta la struttura agli strumenti realmente generati da Tauri, senza duplicare inutilmente crate o configurazioni.

### Livelli

- **Domain:** entità, value object, regole, eventi di dominio; niente dipendenze dalla UI.
- **Application:** casi d'uso e orchestrazione transazionale.
- **Infrastructure:** SQLite, filesystem, motore di ricerca, provider AI, calendari e servizi esterni.
- **Presentation:** frontend e comandi Tauri validati.
- **Platform:** notifiche, keyring, percorsi, avvio automatico e integrazioni native.

### Confini di sicurezza Tauri

- Definisci capability e permessi minimi necessari.
- Non esporre API filesystem o shell generiche al frontend.
- Convalida lato Rust ogni input ricevuto dal frontend.
- Non concatenare input utente in query SQL: usa query parametrizzate.
- Applica CSP restrittiva e limita origini remote.
- Evita `eval` e HTML non sanificato.
- I comandi Tauri devono essere piccoli, tipizzati e autorizzati.
- Nessun segreto nel frontend, bundle, repository o log.

---

## 4. Modello dati

Progetta uno schema relazionale con UUID, date UTC e timestamp di creazione/modifica. Usa cancellazione logica dove utile e definisci chiaramente il comportamento di eliminazione definitiva.

Entità minime:

### `notes`
- `id`
- `notebook_id` nullable, secondo le regole del prodotto
- `title`
- `content` (documento strutturato serializzato in formato versionato)
- `content_plaintext` o rappresentazione indicizzabile
- `created_at`, `updated_at`
- `archived_at`, `deleted_at`
- `is_pinned`
- `source` / metadati d'importazione opzionali
- `sync_status` e `revision` se necessari per la sincronizzazione

### `notebooks`
- `id`, `name`, `description`
- `space_id`
- `created_at`, `updated_at`
- `deleted_at`, `sort_order`

### `spaces`
- `id`, `name`, `description`
- `created_at`, `updated_at`, `sort_order`

### `tags`
- `id`, `name`, `color` opzionale
- unicità del nome secondo lo spazio di appartenenza scelto
- `created_at`, `updated_at`

### `note_tags`
- relazione molti-a-molti tra note e tag, con vincoli di integrità

### `attachments`
- `id`, `note_id`, `original_filename`, `stored_filename`
- `mime_type`, `size_bytes`, `sha256`
- `created_at`, `deleted_at`
- il percorso deve essere relativo alla directory dati controllata dall'app

### `tasks`
- `id`, `note_id` opzionale, `title`, `description`
- `status`, `priority`, `due_at`, `completed_at`
- `reminder_at`, `assignee_id` opzionale
- `created_at`, `updated_at`, `deleted_at`

### `reminders`
- `id`, `entity_type`, `entity_id`, `remind_at`
- `status`, `last_triggered_at`, `created_at`

### `note_links`
- `source_note_id`, `target_note_id`, `label` opzionale

### `events`
- `id`, `title`, `description`, `start_at`, `end_at`, `timezone`
- `calendar_provider`, `external_id` opzionali
- `created_at`, `updated_at`

### `settings`
- chiavi e valori versionati; mai memorizzare segreti non cifrati

### `sync_outbox` e `sync_state`
- preparare eventi di modifica idempotenti e cursori di sincronizzazione solo quando la sincronizzazione sarà implementata.

### AI e ricerca

- metadati degli embedding: entità, modello, versione, data di aggiornamento;
- stato di indicizzazione;
- preferenze di privacy per provider e singole funzioni;
- cronologia chat AI opzionale, disattivabile e cancellabile.

Definisci indici, foreign key, vincoli unici e comportamento `ON DELETE` esplicito. La ricerca full-text deve aggiornarsi coerentemente quando una nota viene creata, modificata, archiviata o eliminata. Le migrazioni devono essere testate anche su database esistenti.

### Formato del contenuto

Usa un formato editoriale strutturato e versionato, preferibilmente basato su JSON/AST del rich-text editor scelto, con schema documentato e conversione in testo semplice/HTML sicuro/Markdown. Non usare HTML arbitrario come unica fonte di verità. Le migrazioni del formato contenuto devono essere versionate.

---

## 5. UX e layout

Realizza un'interfaccia moderna, pulita e rapida, non una copia visiva di Evernote.

### Layout desktop

- barra laterale sinistra ridimensionabile;
- navigazione: Tutte le note, Preferite, Recenti, Attività, Promemoria, Taccuini, Spazi, Tag, Cestino;
- campo di ricerca sempre facilmente accessibile;
- lista centrale di note con ordinamento, anteprima e filtri;
- editor nella zona principale;
- pannello dettagli per tag, taccuino, date, allegati e promemoria;
- barra di stato con indicazione offline/sincronizzazione e salvataggio;
- menu e dialog accessibili da tastiera;
- modalità chiara e scura;
- layout adattabile a finestre strette.

### Interazioni obbligatorie

- `Ctrl/Cmd+N`: nuova nota;
- `Ctrl/Cmd+F`: ricerca;
- `Ctrl/Cmd+S`: salva/forza flush locale se applicabile;
- `Ctrl/Cmd+Shift+N`: nuovo taccuino;
- `Ctrl/Cmd+Z` / `Ctrl/Cmd+Shift+Z`: undo/redo nell'editor;
- `Esc`: chiusura dei dialog ove appropriato;
- focus e scorciatoie non devono interferire con l'input dell'editor.

Mostra stati di caricamento, vuoto, errore, successo e conferma per operazioni distruttive. Evita toast che scompaiono troppo rapidamente per gli errori importanti.

### Editor

Scegli un editor rich-text web maturo e compatibile con Tauri, per esempio Tiptap/ProseMirror o Lexical, dopo aver valutato licenze, accessibilità, esportazione e supporto alle funzionalità. Isola l'editor dietro un'interfaccia applicativa per poterlo sostituire.

Funzioni:
- testo semplice e rich text;
- titoli e intestazioni;
- grassetto, corsivo, sottolineato, barrato;
- elenchi puntati e numerati;
- checklist;
- citazioni;
- collegamenti;
- tabelle di base;
- separatori;
- codice inline e blocchi di codice;
- immagini e allegati;
- trascinamento file nell'editor;
- undo/redo;
- salvataggio automatico con debounce e flush alla chiusura;
- indicatore “Salvato” / “Salvataggio…” / “Errore di salvataggio”;
- recupero di bozze in caso di chiusura inattesa;
- gestione conflitti per modifiche concorrenti quando sarà disponibile la sincronizzazione.

Non perdere modifiche quando si cambia nota, si chiude la finestra o si verifica un errore di rete.

---

## 6. Funzionalità di base — priorità P0

Queste sono il nucleo del prodotto e vanno implementate prima delle funzioni AI o delle integrazioni complesse.

### Note
- creare, leggere, modificare, duplicare, archiviare, ripristinare ed eliminare note;
- cestino e cancellazione definitiva con conferma;
- titoli automatici iniziali modificabili;
- note preferite/pinnate;
- elenco recenti;
- ordinamento per data, titolo e taccuino;
- salvataggio automatico affidabile;
- collegamenti interni tra note;
- cronologia revisioni locale, almeno per le versioni principali, oppure snapshot configurabili.

### Taccuini, spazi e tag
- CRUD completo;
- spostamento note;
- selezione multipla e operazioni batch;
- tag multipli per nota;
- filtro per combinazioni di tag;
- gestione di taccuini vuoti e duplicati;
- ordinamento manuale e drag-and-drop accessibile.

### Allegati
- allegare file con dialog nativo;
- trascinare file;
- mostrare nome, tipo, dimensione e data;
- aprire con applicazione predefinita in modo sicuro;
- rinominare o rimuovere allegati;
- prevenire path traversal, nomi duplicati e scrittura fuori dalla directory dell'app;
- limite dimensioni configurabile e messaggi comprensibili;
- hash per rilevare corruzione/duplicati.

### Offline e persistenza
- tutte le funzioni P0 devono funzionare senza rete;
- il database deve essere in un percorso dati utente appropriato per Linux e Windows;
- nessun dato di nota deve essere cancellato per errori transitori;
- backup manuale e automatico configurabile;
- verifica integrità e procedura di ripristino;
- migrazioni atomiche e backup preventivo prima delle migrazioni rischiose.

---

## 7. Ricerca avanzata

### P0
- ricerca full-text locale per titolo, testo e contenuti indicizzabili degli allegati quando possibile;
- risultati rapidi, ordinati per rilevanza e data;
- evidenziazione dei termini trovati;
- filtri per taccuino, spazio, tag, data, preferite, archiviate e con allegati;
- ricerca tra note attive; includere cestino/archivio solo su richiesta.

### Sintassi query proposta

Implementa un parser con grammatica documentata, senza costruire SQL direttamente dal testo utente. Supporta inizialmente:

- `tag:progetto`
- `notebook:personale`
- `space:lavoro`
- `created:2026-01-01..2026-12-31`
- `updated:7d`
- `has:attachment`
- `is:pinned`
- `is:archived`
- `is:task`
- `before:2026-10-01`
- `after:2026-01-01`
- frasi tra virgolette;
- operatori `AND`, `OR`, `NOT` con precedenza esplicita e parentesi, se il parser completo viene testato.

Gestisci caratteri speciali, query malformate e input molto lunghi. Mostra un errore leggibile invece di fallire silenziosamente.

### Ricerca semantica — P2
- embedding generati da un modello locale o da un provider remoto configurato;
- indice vettoriale aggiornabile e ricostruibile;
- filtri tradizionali combinabili con similarità semantica;
- mostrare perché un risultato è stato suggerito, ove possibile;
- rispettare le esclusioni privacy;
- eliminare e rigenerare gli embedding quando cambia modello o nota;
- non inviare mai contenuti remoti senza informare l'utente e ricevere consenso configurabile.

---

## 8. Tasks e promemoria

### Tasks — P1
- attività autonome o collegate a una nota;
- titolo, descrizione, stato, priorità, scadenza e completamento;
- visualizzazione “Oggi”, “In ritardo”, “Prossime”, “Completate”;
- attività ricercabili;
- collegamento rapido alla nota di origine;
- assegnazione ad altri utenti solo quando esiste un sistema account/sync reale: non simulare utenti remoti nella versione locale.

### Promemoria — P1
- promemoria su note e task;
- data/ora e fuso orario gestiti correttamente;
- notifiche desktop tramite meccanismi supportati dal sistema;
- persistenza anche quando l'app è chiusa, se il sistema lo consente e il servizio è configurato;
- possibilità di rinviare, completare o cancellare;
- permessi e comportamento documentati per Linux e Windows;
- test per cambio fuso, cambio ora legale e riavvio del sistema.

---

## 9. Calendario — P2

- vista calendario mensile e settimanale;
- creazione, modifica e cancellazione di eventi locali;
- collegamento facoltativo a Google Calendar e Microsoft 365/Outlook;
- OAuth con redirect URI e permessi minimi;
- gestione rinnovo/revoca token e cancellazione delle credenziali;
- sincronizzazione bidirezionale solo dopo aver definito la risoluzione dei conflitti;
- distinguere eventi locali da eventi esterni;
- non memorizzare password del provider;
- non dichiarare l'integrazione operativa finché OAuth, API, gestione errori e test non funzionano.

Le API e le condizioni di Google e Microsoft cambiano: verifica la documentazione ufficiale durante l'implementazione. Non assumere che Gmail, Drive, Outlook o Calendar siano integrabili senza configurazioni e autorizzazioni.

---

## 10. Importazione ed esportazione — P1

### Esportazione
- singola nota in Markdown, HTML sicuro, testo semplice e PDF;
- taccuino in archivio ZIP con Markdown e allegati;
- esportazione completa in formato aperto documentato;
- mantenere tag, date, gerarchie, link e allegati quanto possibile;
- nomi file sanitizzati e gestione collisioni;
- PDF con layout leggibile, paginazione, caratteri Unicode e immagini;
- anteprima o conferma prima di esportazioni massive.

### Importazione
- Markdown e cartelle di Markdown;
- testo semplice e HTML sanificato;
- archivio di esportazione dell'app;
- eventuali formati Evernote (ENEX) solo dopo aver verificato il formato e realizzato un parser con test e campioni; non dipendere da API private;
- anteprima, conteggio elementi e scelta di destinazione;
- modalità “simulazione”/dry-run;
- rilevamento duplicati;
- report di elementi importati, saltati e falliti;
- rollback o importazione in un taccuino temporaneo per evitare import parziali incontrollati.

### Email
L'invio di note via email può essere aggiunto come integrazione configurabile con provider SMTP/API, ma non deve essere obbligatorio per l'uso locale. Proteggi credenziali e consenti all'utente di verificare destinatario e contenuto prima dell'invio.

---

## 11. Funzioni AI — P2/P3

Le funzioni AI devono essere modulari, disattivabili e rispettose della privacy. Prima implementa un'interfaccia provider indipendente, poi i provider concreti.

### Interfaccia provider
Definisci un trait/API comune con:
- elenco modelli e capacità;
- completamento chat;
- streaming, se supportato;
- embedding;
- timeout, cancellazione e gestione rate limit;
- limiti dimensione input/output;
- metadati di provenienza;
- errori tipizzati;
- opzione per provider locale o remoto.

### Provider
- **locale:** integrazione con server locale compatibile con API OpenAI, per esempio Ollama, configurabile per URL e modello;
- **remoto:** provider compatibili con API documentate, con chiavi salvate nel keyring;
- possibilità di disattivare completamente l'AI.

Non assumere che Ollama o un altro servizio sia installato. Mostra un test di connessione e istruzioni di configurazione. Non scaricare modelli grandi senza consenso.

### Assistente AI sulle note
- chat con contesto scelto dall'utente;
- ricerca di note rilevanti e citazione delle note usate, con link interni;
- sintesi e organizzazione;
- proposta di tag;
- estrazione di attività e date;
- risposta basata sulle note personali, distinguendo contenuto recuperato da conoscenza generale;
- non modificare o cancellare note autonomamente senza conferma;
- azioni con anteprima e annullamento ove possibile.

### Modifica AI
- correggi grammatica;
- riscrivi con tono scelto;
- riassumi;
- espandi;
- traduci;
- crea elenco di punti;
- estrai decisioni e azioni;
- confronta versioni;
- mostra un diff/anteprima prima di sostituire il contenuto.

### Trascrizione di riunioni — P3
- registrazione solo dopo un'azione esplicita e visibile;
- indicatore persistente durante la registrazione;
- selezione dispositivo e test microfono;
- trascrizione con motore locale o provider configurato;
- riepilogo e action item;
- consenso e obblighi legali da 
rispettare;
- controllo per cancellare audio e trascrizione;
- niente registrazione in background nascosta.

### RAG / retrieval
- pipeline: suddivisione in chunk, indicizzazione, embedding, retrieval e composizione del contesto;
- chunk con riferimenti alla nota e alla posizione originale;
- filtri per spazio/taccuino/tag e permessi;
- evitare di includere nel contesto note escluse o nel cestino;
- difese contro prompt injection contenuta nelle note: trattare il contenuto recuperato come dati non attendibili, non come istruzioni;
- citazioni verificabili alle note sorgente;
- valutazioni con un piccolo dataset sintetico e casi di test;
- cache locale invalidabile;
- trasparenza su quali note vengono inviate a provider remoti.

### Pulizia AI
Funzione opzionale per migliorare la formattazione senza alterare il significato. Mostrare confronto prima/dopo e richiedere conferma.

---

## 12. Sincronizzazione multi-dispositivo — P3

Questa è una funzione complessa: non fingere che il semplice upload del database SQLite sia sincronizzazione.

Progetta un protocollo indipendente dal provider, con:
- identità utente/dispositivo;
- autenticazione e revoca dei dispositivi;
- API versionate;
- coda locale outbox/inbox;
- operazioni idempotenti;
- revisioni, cursori e retry con backoff;
- rilevamento e risoluzione conflitti;
- gestione di cancellazioni/tombstone;
- sincronizzazione degli allegati con hash e ripresa;
- migrazione schema client/server;
- diagnostica e stato visibile;
- test di disconnessione, duplicati, riordino eventi e conflitti.

### Regole sui conflitti
- non usare automaticamente “vince l'ultimo timestamp” come unica strategia;
- non perdere silenziosamente contenuti;
- per modifiche concorrenti alla stessa nota, conservare le versioni e offrire confronto/risoluzione;
- progettare le operazioni affinché un retry non duplichi note o task.

### Backend futuro
Per la sincronizzazione cloud servirà un servizio separato, autenticazione, database server e storage allegati. Non inserire un server remoto fittizio nel client. Documenta un possibile backend Rust con API HTTP e PostgreSQL o soluzione equivalente, senza implementarlo prima che il client locale sia stabile.

### Crittografia
- TLS per traffico di rete;
- segreti nel keyring;
- valutare cifratura end-to-end solo con modello di chiavi, recupero account, ricerca e sincronizzazione definiti in modo rigoroso;
- non chiamare “end-to-end encrypted” una cifratura che il server può decifrare;
- documentare limiti e metadati non cifrati.

---

## 13. Sicurezza e privacy

- directory dati con permessi restrittivi;
- query SQL parametrizzate;
- convalida MIME e dimensioni degli allegati; non fidarsi dell'estensione;
- protezione path traversal e symlink;
- sanificazione HTML importato e contenuti visualizzati;
- protezione contro link `file://` o schemi non sicuri;
- apertura allegati tramite API di sistema con conferma quando necessario;
- token nel keyring o in storage cifrato approvato;
- redazione di note, token e dati sensibili dai log;
- blocco dei segreti nel frontend e negli errori;
- backup con opzione di cifratura e password gestita con primitive robuste;
- cancellazione dei dati e degli embedding su richiesta;
- telemetria disabilitata per impostazione predefinita; se aggiunta, opt-in esplicito e documentato;
- dipendenze controllate e aggiornate; audit delle dipendenze quando praticabile;
- report di vulnerabilità e procedura di aggiornamento;
- threat model documentato in `docs/SECURITY.md`.

Non implementare una crittografia proprietaria. Usa librerie e protocolli consolidati, con revisione accurata.

---

## 14. Integrazioni ed estensioni

Architettura predisposta per:
- MCP server per esporre note e strumenti AI;
- Gmail/Google Drive/Google Calendar;
- Outlook/Microsoft 365;
- Slack;
- eventuali API REST locali.

### MCP
- iniziare con un server MCP locale opzionale;
- strumenti a sola lettura come default: ricerca note, leggi nota, elenca taccuini e tag;
- strumenti di modifica separati e disabilitati finché l'utente non li abilita;
- conferma per operazioni distruttive;
- scope e permessi minimi;
- autenticazione/controllo accessi se raggiungibile via rete;
- evitare di esporre il server su `0.0.0.0` per default;
- audit log delle azioni, senza registrare il contenuto integrale delle note;
- difese contro prompt injection e input non attendibile.

### Sistema di plugin
Non realizzare subito un sistema di plugin arbitrari eseguibili con accesso completo. Iniziare con interfacce interne e integrazioni esplicite; in seguito valutare un modello di capability con permessi per singola estensione.

---

## 15. Account, condivisione ed enterprise

Queste funzionalità richiedono un backend reale e sono successive alla modalità locale.

- account e autenticazione;
- MFA/2FA con flussi di recupero sicuri;
- condivisione di note e taccuini con ruoli;
- assegnazione task ad altri utenti;
- amministrazione centralizzata;
- ruoli e permessi;
- audit log;
- SSO OIDC/SAML tramite provider adeguato;
- policy di retention e cancellazione;
- gestione tenant e separazione dati;
- esportazione e cancellazione dell'account;
- fatturazione solo se il prodotto diventa un servizio commerciale.

Non implementare una schermata di login che non protegga realmente i dati. Non inventare utenti o permessi server-side nel client locale.

---

## 16. Accessibilità e localizzazione

- interfaccia in italiano come lingua iniziale;
- struttura i18n con chiavi, non stringhe duplicate nei componenti;
- predisposizione inglese;
- formati data/ora/numero localizzati;
- supporto tastiera e focus visibile;
- etichette accessibili per icone e controlli;
- dimensioni testo e contrasto adeguati;
- non affidare il significato esclusivamente al colore;
- test con finestra ridotta e scaling elevato.

---

## 17. Prestazioni e robustezza

Obiettivi iniziali da misurare, non garanzie da assumere:
- avvio rapido su un laptop Linux comune;
- UI reattiva durante ricerca, importazione ed elaborazioni AI;
- operazioni pesanti fuori dal thread UI;
- paginazione o virtualizzazione di liste grandi;
- indice di ricerca aggiornato in modo incrementale;
- nessun freeze prolungato durante l'indicizzazione;
- supporto a decine di migliaia di note attraverso test sintetici;
- import/export con progressione e cancellazione ove possibile;
- consumo memoria proporzionato alle note aperte, non all'intero archivio.

Aggiungi metriche locali di sviluppo e benchmark ripetibili, senza inviare telemetria all'esterno.

---

## 18. Packaging Linux e futuro Windows

### Linux — P0
- installazione e avvio da build di sviluppo;
- pacchetto `.deb` e AppImage quando compatibili con lo stack scelto;
- icona e file desktop;
- percorsi dati XDG;
- test su almeno una distribuzione Debian/Ubuntu-based e una Fedora-based, se l'ambiente lo consente;
- dipendenze runtime documentate;
- disinstallazione senza cancellare automaticamente i dati dell'utente;
- aggiornamenti e migrazioni sicuri.

### Windows — P3
- build x64 tramite pipeline CI;
- installer `.msi` o `.exe` secondo il packaging Tauri;
- percorsi dati Windows corretti;
- notifiche, keyring e apertura file verificati su Windows;
- test con Windows Defender e path con spazi/caratteri Unicode;
- nessun comando shell specifico Linux nella logica di dominio.

### CI
Configura GitHub Actions o altro CI se il repository lo usa:
- lint frontend;
- check, clippy e test Rust;
- build Linux;
- build Windows quando runner e dipendenze lo permettono;
- scansione dipendenze;
- artefatti di release versionati.

Non pubblicare automaticamente release firmate senza configurare e proteggere le credenziali.

---

## 19. Test e qualità

### Test unitari
- regole dominio;
- parser della ricerca;
- normalizzazione tag;
- gestione date e fusi;
- conversione contenuto;
- policy di permessi;
- logica di sync e conflitti quando introdotta.

### Test di integrazione
- CRUD e migrazioni SQLite;
- transazioni e rollback;
- aggiornamento FTS;
- allegati e ripristino;
- import/export round-trip;
- backup e restore;
- errori disco/permessi;
- provider AI con server finto nei test;
- OAuth con callback simulato, senza credenziali reali.

### Test UI/E2E
- creare, modificare e ritrovare una nota;
- riavviare e verificare la persistenza;
- spostare una nota tra taccuini;
- aggiungere e rimuovere tag;
- allegare ed esportare un file;
- usare filtri di ricerca;
- verificare scorciatoie e accessibilità di base;
- verificare che la UI segnali gli errori di salvataggio;
- confermare che funzioni P0 non richiedano rete.

### Test di sicurezza
- SQL injection;
- HTML/script malevolo importato;
- path traversal;
- file con MIME/estensioni ingannevoli;
- token nei log;
- prompt injection in note recuperate dall'AI;
- permessi MCP e accesso non autorizzato.

Non usare dati reali dell'utente nei test. Usa fixture sintetiche.

---

## 20. Roadmap obbligatoria

Procedi in quest'ordine, mantenendo `ROADMAP.md` aggiornato con checkbox e criteri di accettazione.

### Fase 0 — Ricognizione e bootstrap
- controlla toolchain e repository;
- crea struttura, documentazione, lint e CI iniziale;
- genera uno scheletro Tauri avviabile;
- definisci design system e gestione configurazione;
- documenta le prime decisioni architetturali.

**Accettazione:** l'app si avvia su Linux; build e test di base passano.

### Fase 1 — Core locale
- schema SQLite e migrazioni;
- repository e casi d'uso;
- CRUD note;
- taccuini, spazi, tag;
- editor rich-text;
- autosave e recupero errori;
- layout principale e temi.

**Accettazione:** creare, modificare, organizzare e ritrovare note dopo riavvio, tutto offline.

### Fase 2 — Ricerca e produttività
- FTS5 e filtri;
- parser query;
- preferite, archivio, cestino e recenti;
- allegati;
- link interni;
- scorciatoie.

**Accettazione:** test di ricerca e allegati passano; nessuna query costruita concatenando input.

### Fase 3 — Import/export e protezione dati
- Markdown, testo, HTML sanificato;
- export Markdown/HTML/PDF/ZIP;
- backup/restore e verifica integrità;
- guide utente e recupero dati.

**Accettazione:** round-trip dei formati supportati e backup ripristinabile verificati.

### Fase 4 — Task, promemoria e calendario locale
- task;
- notifiche;
- calendario eventi locali;
- test su riavvio, fuso e ora legale.

**Accettazione:** scadenze e promemoria persistono e sono visualizzati correttamente.

### Fase 5 — AI locale/remota e ricerca semantica
- interfaccia provider;
- connessione a provider locale configurabile;
- chat con retrieval e fonti;
- modifica AI con anteprima;
- embedding e ricerca semantica;
- privacy settings.

**Accettazione:** l'AI è facoltativa, cita le note usate e non modifica il contenuto senza conferma.

### Fase 6 — Integrazioni
- MCP locale sicuro;
- import ENEX se fattibile;
- Google/Microsoft/Slack secondo API e autorizzazioni disponibili.

**Accettazione:** ogni integrazione ha test, documentazione di configurazione e gestione revoca/errori.

### Fase 7 — Sincronizzazione
- progettazione protocollo;
- backend reale;
- autenticazione dispositivi;
- sync incrementale, conflitti e allegati.

**Accettazione:** test di concorrenza e perdita di rete non perdono dati; nessun mock viene presentato come sync reale.

### Fase 8 — Porting e rilascio
- packaging Linux;
- CI multipiattaforma;
- build e test Windows;
- hardening sicurezza;
- guida installazione/aggiornamento;
- checklist release.

**Accettazione:** pacchetti installabili e verificati su sistemi supportati, con procedura di backup e rollback.

---

## 21. Definition of Done generale

Il prodotto non è “completo” solo perché compila. Per ogni release:
- build riproducibile per le piattaforme dichiarate;
- suite test verde;
- lint senza errori bloccanti;
- migrazioni verificate;
- backup e restore provati;
- documentazione aggiornata;
- nessun segreto committato;
- nessun pulsante P0 privo di implementazione;
- nessun errore critico noto nascosto;
- funzionalità non disponibili esplicitamente indicate;
- procedura di aggiornamento e recupero dati testata.

---

## 22. Note operative del repository

- Un solo repository per Linux e Windows (ADR 0024): niente fork per
  piattaforma. Le differenze vanno in `src-tauri/tauri.<os>.conf.json`,
  negli script di build e in blocchi `cfg`.
- Il motore AI integrato è lo stesso su tutte le piattaforme:
  `scripts/fetch-llama.ps1` (Windows) e `scripts/fetch-llama.sh` (Linux)
  usano la stessa build di llama.cpp. Se cambi versione, aggiorna entrambi.
- L'endpoint degli aggiornamenti è `tihcio/greatnote`. Non creare un
  repository chiamato `greatnote-win`: romperebbe il reindirizzamento per
  le installazioni 0.1.x.

## 23. Primo incarico da eseguire adesso

Inizia **subito** dalla Fase 0, non fermarti a riscrivere questa specifica.

1. Esamina il repository e l'ambiente disponibile.
2. Elenca brevemente lo stato rilevato e le eventuali limitazioni reali.
3. Crea `ROADMAP.md` con le fasi e le checkbox.
4. Crea `ARCHITECTURE.md` e i primi ADR.
5. Inizializza il progetto Tauri + Rust + frontend TypeScript, se l'ambiente lo permette.
6. Implementa una prima finestra desktop con layout base, navigazione laterale e schermata note vuota.
7. Prepara configurazione, schema iniziale e primo test; non simulare persistenza se non è ancora implementata.
8. Esegui i controlli disponibili e correggi gli errori.
9. Aggiorna `README.md` con i comandi esatti per installare dipendenze, avviare in sviluppo, testare e compilare su Linux.
10. Alla fine, riporta file creati/modificati, comandi eseguiti, risultati dei test, limitazioni e il prossimo passo concreto.

**Importante:** dopo la Fase 0 continua con la Fase 1 quando possibile. Non aspettare un'ulteriore conferma per le normali decisioni tecniche reversibili. Se l'ambiente non consente di compilare o installare pacchetti, implementa comunque la parte possibile, annota l'impedimento esatto e fornisci i comandi riproducibili per proseguire.
