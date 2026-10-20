# ADR 0010: AI facoltativa, provider compatibili OpenAI e ricerca semantica locale

- **Stato:** accettata
- **Data:** 2026-10-19

## Contesto

La Fase 5 aggiunge assistente sulle note, modifiche assistite, suggerimenti
e ricerca per significato. Il progetto richiede che l'AI sia facoltativa e
disattivabile, privilegi i modelli locali, non invii contenuti remoti senza
consenso, citi le note usate, non modifichi nulla senza conferma, tenga le
chiavi fuori da file e log e tratti il contenuto delle note come dato non
attendibile (prompt injection).

## Decisione

### Provider

- Un trait `AiProvider` (`src/ai/provider.rs`) descrive elenco modelli, chat
  in streaming ed embedding, con annullamento, tempo massimo ed errori
  tipizzati (`AiError`: connessione, tempo scaduto, 401/403, 429 con
  `Retry-After`, errori HTTP con messaggio accorciato, risposte non valide).
- Unica implementazione: `OpenAiCompatible` (`/models`, `/chat/completions`
  con SSE, `/embeddings`), che copre Ollama, LM Studio, llama.cpp server e i
  servizi remoti compatibili. Nessun SDK proprietario.
- Client HTTP `ureq` 3 (bloccante, rustls, MIT/Apache-2.0): le chiamate
  girano in thread separati dalla UI. Nessun reindirizzamento seguito (la
  chiave non deve finire su un altro host); il proxy di sistema è ignorato
  per i server locali. Il ragionamento `<think>…</think>` di alcuni modelli
  locali è rimosso dall'output.
- Nessun modello viene scaricato: l'utente indica modelli già installati e
  può provare la connessione (che chiede solo l'elenco dei modelli).

### Privacy e consenso

- Impostazioni in `settings` (`ai.settings`), **disattivate per
  impostazione predefinita**. Un indirizzo non loopback deve usare `https` e
  richiede il consenso esplicito (`remote_consent`); senza, ogni comando che
  invia dati viene rifiutato nel backend.
- Con un server remoto ogni invio è mostrato prima: per la chat l'elenco dei
  brani che verranno inviati (deselezionabili), per le modifiche il testo,
  per tag e attività la nota. Con un server locale l'invio è diretto e le
  fonti sono mostrate con la risposta.
- Taccuini esclusi: le loro note non vengono mai inviate né indicizzate
  (filtro nel backend su recupero, risoluzione delle fonti, comandi sulla
  nota e indicizzazione).
- La cronologia delle richieste (migrazione v10, aggiunta il 2026-10-19) è
  facoltativa, attiva per impostazione predefinita perché resta solo sul
  dispositivo, limitata a 1.000 voci e cancellabile; delle fonti conserva
  solo nota e titolo.
- La chiave API è nel portachiavi di sistema tramite `keyring` 4
  (Secret Service su Linux, Credential Manager su Windows); nel database
  resta solo il flag `use_api_key`. La chiave non torna mai al frontend e
  non compare in `Debug`/log. Se il portachiavi non è disponibile la chiave
  non viene salvata altrove.

### Recupero e citazioni

- La chat è in due passi: `ai_prepare_chat` trova le fonti (indice
  semantico se c'è un modello di embedding, altrimenti FTS5 con termini
  normalizzati, tra virgolette e in OR, passati come parametro); `ai_chat`
  riceve **solo riferimenti** (nota, posizione) e rilegge il testo dalla
  nota corrente, verificando di nuovo cestino ed esclusioni.
- Le fonti sono numerate; il modello deve citarle come `[n]` e il frontend
  rende cliccabili solo i numeri di fonti realmente inviate. L'elenco delle
  fonti con estratto e metodo (significato, parole chiave, nota scelta) è
  sempre visibile: le citazioni sono verificabili.
- Un budget di caratteri (`max_context_chars`) limita il testo inviato.

### Difese contro la prompt injection

- Il contenuto delle note e il testo da elaborare stanno tra delimitatori
  (`<fonte>`, `<testo>`) che il contenuto non può falsificare: le sue
  occorrenze dei tag vengono neutralizzate (`neutralize_delimiters`).
- Le istruzioni di sistema dichiarano quel contenuto dato non attendibile.
- Il modello non ha strumenti: non può leggere altre note, chiamare comandi
  o modificare dati. Ogni risultato è testo mostrato all'utente.
- Le risposte strutturate (tag, attività) sono analizzate in modo
  difensivo: primo array JSON, valori ripuliti e limitati, date verificate.

### Modifiche con anteprima

- Correggi, riscrivi con tono, riassumi, espandi, traduci, elenco di punti,
  decisioni e azioni, migliora formattazione: il risultato arriva in
  streaming in un dialog con confronto per parole (diff) e viene applicato
  solo con "Sostituisci" o "Inserisci sotto" nell'editor, quindi annullabile
  con Ctrl+Z. Il Markdown restituito è convertito con l'importatore esistente
  e sanificato come ogni documento.
- Tag e attività proposti sono modificabili e vanno confermati uno per uno.

### Indice semantico

- Migrazione v9: `ai_index_notes` (modello, data di modifica e impronta
  SHA-256 di titolo e testo della versione indicizzata) e `ai_chunks`
  (posizioni nel testo e vettori `f32` normalizzati, **senza copie del
  testo**). Tutto è in `ON DELETE CASCADE` con la nota.
- Una nota è usata solo se la data di modifica coincide con quella
  indicizzata; se cambiano solo i metadati (fissata, spostata) l'impronta
  evita di ricalcolare gli embedding.
- Ricerca per similarità del coseno con scansione completa: adeguata a
  decine di migliaia di frammenti, sostituibile con un indice approssimato.
- Indicizzazione manuale o automatica (opzione, predefinita spenta) in un
  thread, con avanzamento ed eventi, annullabile. Errori del server
  (modello assente, embedding non supportati) fermano il ciclo; testo
  rifiutato per una singola nota viene contato e il ciclo prosegue.
- Cambiare modello o escludere un taccuino elimina i vettori non più
  validi; "Elimina l'indice" li cancella tutti.

## Conseguenze

- Nuove dipendenze: `ureq` (core), `keyring` (shell).
- Le note archiviate sono escluse dall'AI come dalla ricerca predefinita.
- Non implementati in questa fase: confronto AI tra versioni, trascrizione
  di riunioni (P3), pulizia AI dell'intera libreria,
  provider con API non compatibili OpenAI.
- Il test `tests/ai_live.rs` (ignorato di default) verifica un server locale
  reale con note sintetiche; tutti gli altri test usano server HTTP finti.
