# ADR 0011: importazione da Evernote, Joplin e Obsidian

- **Stato:** accettata
- **Data:** 2026-10-19

## Contesto

L'utente vuole portare in Greatnote taccuini e note da Joplin, Evernote e
Obsidian. Il progetto richiede formati verificati, parser testati con
campioni, nessuna dipendenza da API private, anteprima e dry-run, rilevamento
duplicati, report e nessun import parziale incontrollato.

## Decisione

### Formati supportati

- **Evernote:** file **ENEX** (esportazione documentata di Evernote, uno per
  taccuino; se ne possono scegliere più insieme). Il nome del file diventa
  il taccuino. Letti in streaming con `quick-xml` (MIT), che non risolve DTD
  né entità esterne (niente XXE). Il contenuto ENML passa dalla conversione
  HTML sanificata esistente; `<en-media>` è collegato alla risorsa tramite
  l'hash MD5 (`md-5`, usato solo per l'abbinamento, non per sicurezza);
  `<en-todo>` diventa ☐/☑; `<en-crypt>` non viene decifrato (avviso).
  Date ENEX `AAAAMMGGTHHMMSSZ` convertite in UTC.
- **Joplin:** archivio **JEX** (tar con un file `<id>.md` per elemento e
  le risorse in `resources/`), letto in memoria con `tar` (MIT/Apache) entro
  i limiti dell'importazione e senza estrarre nulla su disco. Si leggono
  cartelle annidate, tag (`type_` 5 e 6), risorse, date utente, note
  Markdown e HTML; i link `:/id` a note diventano collegamenti, quelli a
  risorse allegati.
- **Obsidian:** cartella del **vault**. Le cartelle diventano taccuini (le
  note alla radice vanno nel taccuino con il nome del vault), il nome del
  file è il titolo. Front matter YAML (`tags`, `aliases`, `created`,
  `updated`, elenchi su una o più righe), `#tag` nel testo fuori dal codice,
  `[[nota]]`, `[[nota|alias]]`, `![[file]]`, link e immagini Markdown
  relativi. Cartelle nascoste (`.obsidian`, `.trash`) ignorate; date dal
  front matter o dal file system.

### Struttura e collegamenti

- Il piano d'importazione esistente (`ImportPlan`) porta per ogni nota il
  percorso del taccuino d'origine, le chiavi con cui può essere citata e i
  collegamenti in uscita. Anteprima e commit restano gli stessi: dry-run
  senza modifiche, poi un'unica transazione con rimozione dei file scritti in
  caso di errore.
- Nuova destinazione `keep-structure`: un taccuino per ogni taccuino o
  cartella d'origine, facoltativamente in un nuovo spazio (proposto con il
  nome della raccolta). Greatnote ha taccuini piatti negli spazi: le
  cartelle annidate diventano taccuini con il percorso nel nome
  ("Lavoro › Progetti"). Senza spazio, un taccuino con lo stesso nome già
  presente fuori dagli spazi viene riutilizzato. Restano disponibili le
  destinazioni piatte (nuovo taccuino, taccuino esistente, nessuno).
- I collegamenti tra note diventano righe di `note_links` dopo l'inserimento
  di tutte le note; il testo del link resta nel contenuto (i link interni
  non sono tra gli schemi consentiti nel documento). In Obsidian i
  collegamenti sono risolti al percorso canonico della nota (esatto,
  relativo alla nota, per nome o per alias).

### Allegati e sicurezza

- Immagini e PDF incorporati diventano allegati della nota e nel testo resta
  un segnaposto "📎 nome" nella posizione originale (l'editor non ha ancora
  immagini in linea). Tipi non supportati dagli allegati (audio, video,
  Office…) vengono saltati con un avviso nel report.
- Obsidian: un file citato è letto solo se, dopo aver risolto `..` e i
  collegamenti simbolici, resta dentro il vault; cartelle e file collegati
  simbolicamente non vengono seguiti.
- Limiti invariati: 5.000 note e 512 MiB per importazione, 25 MiB per
  allegato, 5 MiB per nota Markdown, 8 MiB di ENML per nota Evernote.
- Duplicati: impronta di titolo e testo, come per le altre importazioni.

## Conseguenze

- Nuove dipendenze del core: `quick-xml` e `base64` (già presenti
  nell'albero), `tar`, `md-5`.
- Non supportati: export Joplin "RAW" o Markdown con front matter (usare
  JEX), attività di Joplin (importate come note, con avviso), pile
  (stack) di Evernote (non presenti negli ENEX), note cifrate di Evernote,
  plugin e canvas di Obsidian (`.canvas` ignorati), collegamenti
  `evernote:///`.
- Verifica eseguita il 2026-10-19 in sola anteprima su un backup JEX reale
  (488 note, 0 errori) e un vault Obsidian reale (486 note, 0 errori); i test
  automatici usano solo campioni sintetici.
