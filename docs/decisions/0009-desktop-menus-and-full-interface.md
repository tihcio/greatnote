# ADR 0009: menu completi e interfaccia sul modello delle app di note

- **Stato:** accettata
- **Data:** 2026-10-14

## Contesto

L'utente ha fornito screenshot di Evernote (in `docs/mockup/`, solo in locale
e non pubblicati nel repository) e chiesto
menu, voci e funzioni analoghi, con ogni voce collegata a un'azione reale.
Il progetto vieta di copiare branding o asset e di simulare servizi non
configurati (account, cloud, AI).

## Decisione

### Menu

- Barra dei menu **nell'interfaccia** (`role="menubar"`), non menu nativo
  GTK: stato attivo/disattivato e spunte cambiano con la nota aperta, è
  navigabile da tastiera (frecce, Invio, Esc) e lo stesso modello alimenta
  menu contestuali ed elenco delle scorciatoie.
- Un unico modello (`frontend/src/app/buildMenus.ts`) definisce File,
  Modifica, Visualizza, Nota, Finestra, Strumenti e Aiuto. Da esso derivano
  la barra, il menu contestuale delle note (poi esteso a tutta l'app con
  `openContextMenu` e `ContextMenuHost`, che blocca il menu della
  webview), il gestore delle scorciatoie (`findAction`) e il dialog "Scorciatoie da tastiera": un'azione non può
  esistere in un posto e mancare nell'altro.
- Le scorciatoie native di modifica (annulla, copia, incolla, seleziona
  tutto, Canc) sono solo mostrate: le gestisce il campo attivo. Le
  combinazioni già gestite dall'editor (es. Ctrl+E per il codice) non
  vengono intercettate; fuori dall'editor Ctrl+E crea un evento.
- "Incolla" dal menu legge gli appunti dal processo Rust
  (`tauri-plugin-clipboard-manager`), perché WebKitGTK blocca
  `execCommand("paste")`; il frontend non riceve permessi del plugin.
- Conferme e richieste di testo usano dialog dell'app (`askConfirmation`,
  `askText`): `confirm()` e `prompt()` non sono affidabili in WebKitGTK.

### Voci adattate o omesse

- **Omesse** (richiedono account o servizi non disponibili): Esci
  dall'account, Info account, Condivisi con me, Upgrade, prompt AI,
  registrazione audio, "Converti blocco appunti".
- **"Condividi"** diventa "Condividi come testo" (copia come Markdown o testo
  semplice): nessun servizio di condivisione simulato.
- **"Apri nell'editor Lite"** e **"Cambia vista nota"** diventano le viste
  della nota (normale, editor ampliato, concentrazione).
- **"Fissa nel taccuino/nella Home"**: le scorciatoie (ex preferite) sono
  mostrate nella Home; non c'è un fissaggio per taccuino.
- **Ripeti** nelle attività resta escluso (nessuna ricorrenza).

### Funzioni aggiunte

- Schede con cronologia avanti/indietro per scheda (posizione = sezione,
  vista e nota); nuove schede, chiusura, scorrimento con Ctrl+Tab.
- Finestra separata per una nota (`open_note_window`): etichetta e URL
  derivano da un UUID validato; la finestra principale chiude la nota per
  non modificarla in due punti e riceve l'evento `note-changed`.
- Home, File (tutti gli allegati), Modelli (predefiniti e dell'utente, nel
  cestino come gli altri elementi), cronologia delle revisioni (migrazione
  v8, istantanee al massimo ogni 10 minuti, ultime 50), duplica/copia in
  (con tag e copie indipendenti degli allegati), informazioni nota,
  contatori della barra laterale, attività contrassegnate.
- Preferenze dell'interfaccia (tema chiaro/scuro/sistema, zoom, larghezza
  nota, vista iniziale, opzioni della barra laterale, comandi "/") salvate
  nella tabella `settings` e quindi incluse nei backup. Il tema forzato usa
  `data-theme` sull'elemento radice; i blocchi CSS del tema scuro valgono
  sia per il sistema sia per la scelta esplicita.
- Editor: barra "Inserisci", stili di testo, carattere e dimensione da
  elenco chiuso, colore ed evidenziazione da tavolozza, checklist, tabelle,
  trova e sostituisci con evidenziazione, comandi rapidi "/". Il formato del
  documento accetta i nuovi nodi e contrassegni; gli attributi di stile sono
  in whitelist nella validazione Rust (diventano CSS) e il frontend
  normalizza quelli incollati da altre app prima del salvataggio.

### Test dell'interfaccia

`npm run test:ui` avvia Vite e apre `frontend/test-harness.html` (backend
Tauri simulato con dati sintetici, escluso dalla build) in Firefox headless
tramite `puppeteer-core` e WebDriver BiDi, senza scaricare browser. Esegue i
flussi principali e fallisce su passaggi non riusciti o errori in console.

## Conseguenze

- Nuove dipendenze: `tauri-plugin-clipboard-manager` (shell),
  `@tiptap/extension-table`, `-highlight`, `-text-style`, `-list` (editor),
  `puppeteer-core` (solo sviluppo).
- La finestra separata e le schede condividono lo stesso database: le
  modifiche concorrenti alla stessa nota in due finestre sono evitate
  chiudendola nella principale, non risolte con un algoritmo di merge.
- L'importazione Markdown/HTML non crea ancora checklist e tabelle native
  (restano testo, come prima); l'esportazione invece le conserva.
- Il test dell'interfaccia usa un backend simulato: verifica il frontend, non
  l'integrazione con Rust, coperta dai test del core.
