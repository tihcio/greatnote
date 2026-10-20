# ADR 0013: immagini nel testo e lettura del testo delle immagini

- **Stato:** accettata
- **Data:** 2026-10-19

## Contesto

Le immagini erano solo allegati sotto la nota. L'utente vuole vederle nel
testo e che l'assistente AI possa usare il testo che contengono
(scontrini, lavagne, documenti fotografati).

## Decisione

### Immagini nel testo

- Nuovo nodo `image` nel documento con attributi `attachmentId`
  (obbligatorio), `alt` (≤ 500 caratteri) e `width` (40–2400 px). Nessun
  `src`: un'immagine può solo riferirsi a un allegato della nota. La
  validazione Rust rifiuta qualsiasi altro attributo, quindi un URL esterno
  o un percorso non può entrare nel documento (niente richieste di rete
  dalle note incollate, niente lettura di file arbitrari).
- L'editor (estensione `AttachmentImage`) carica i byte con il comando
  `read_attachment_image`, che accetta solo allegati immagine attivi e ne
  verifica dimensione e SHA-256; il frontend li mostra come `blob:` URL.
- Le immagini aggiunte con il pulsante, trascinate o incollate diventano
  allegati e compaiono subito nel testo; dal pannello allegati si possono
  inserire quelle già presenti.
- Importazioni da Evernote, Joplin e Obsidian: i segnaposto «📎 nome» delle
  immagini incorporate diventano nodi immagine al loro posto (il paragrafo
  viene diviso; nelle voci di elenco il primo paragrafo resta tale).
- Copie di note e archivi con id di allegato rigenerati aggiornano i
  riferimenti nel testo (`remap_image_attachments`).
- Esportazione: Markdown `![alt](<percorso dell'allegato>)` nello ZIP
  Markdown, testo semplice «📎 alt», HTML con il testo alternativo (il PDF
  stampa la nota con le immagini).
- L'impronta dei duplicati ignora gli spazi, così dividere un paragrafo
  attorno a un'immagine non fa sembrare nuova una nota già importata.

### Dimensione e annotazioni (aggiunte il 2026-10-19)

- La larghezza (`width`, 40–2400 px) si imposta trascinando la maniglia o
  con le dimensioni rapide; è un attributo del nodo, quindi si annulla con
  Ctrl+Z.
- Le annotazioni sono un attributo `annotations` del nodo: al massimo 300
  forme vettoriali (`pen`, `highlighter` con 2–2000 punti; `arrow`, `rect`,
  `ellipse` con 2 punti; `text` con 1 punto e fino a 500 caratteri),
  coordinate relative all'immagine e spessori relativi alla larghezza, così
  scalano con il ridimensionamento. La validazione Rust ammette solo questi
  campi (nessun HTML o URL). Vivendo nel documento seguono annulla/ripeti,
  revisioni, copie, archivio e backup senza codice dedicato; l'immagine
  originale non viene mai modificata.
- Sono disegnate in SVG sopra l'immagine (stesse primitive nella nota e
  nell'editor). Le note `text` entrano nel testo semplice («📎 nome — nota»),
  quindi nella ricerca e nel contesto dell'AI, e nelle esportazioni Markdown
  e HTML come testo; la stampa/PDF mostra il disegno.

### Testo nelle immagini (OCR)

- Migrazione v11: `ocr_text`, `ocr_status`, `ocr_engine`, `ocr_error`,
  `ocr_at` sugli allegati. Il testo letto è aggiunto a `search_text`, quindi
  entra nella ricerca full-text, e in coda al testo che l'assistente usa per
  la nota ("Testo nelle immagini: [nome] …"); la nota esce dall'indice
  semantico finché non viene reindicizzata.
- Due motori:
  - **modello AI con visione** del provider configurato: `image_url` con
    data URL per i server compatibili OpenAI (modello di visione facoltativo),
    blocco `image` in input `stream-json` per Claude Code. Valgono consenso,
    taccuini esclusi e limite di 5 MB per immagine;
  - **Tesseract** installato sul computer (locale, nessun invio), letto
    dallo standard input con italiano e inglese se disponibili.
- Prima dell'invio a un modello con visione l'immagine è ridotta (lato
  lungo 1024 px, configurabile 512–2048; JPEG di qualità 90) con limiti di
  decodifica contro file ostili; se il server risponde con un errore di
  contesto insufficiente si riprova a 2/3 e poi a meno della metà. Tesseract
  usa la risoluzione piena.
- Il prompt tratta l'immagine come dato (istruzioni scritte nell'immagine
  ignorate); per immagini senza testo il modello salva una breve
  descrizione, utile per ritrovarle.
- Lettura manuale (pannello allegati), automatica all'aggiunta (opzione) o
  di tutte le immagini in background con avanzamento e annullamento.

## Conseguenze

- Le immagini nel testo non sono ridimensionabili dall'interfaccia (l'attributo
  `width` è già previsto) e i formati restano quelli degli allegati (PNG,
  JPEG, GIF).
- Tesseract non è incluso nell'app: va installato dal sistema.
- La lettura con modello AI non è registrata nella cronologia AI.
- Verificata il 2026-10-19 con Claude Code (`haiku`) su un'immagine
  sintetica: trascrizione esatta in circa 2 s (`tests/ai_live.rs`).
