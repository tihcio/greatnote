# ADR 0017: sincronizzazione tra dispositivi tramite una cartella condivisa

- **Stato:** accettata
- **Data:** 2026-10-20

## Contesto

La Fase 7 chiede di sincronizzare più dispositivi. Le regole della
specifica:

- protocollo indipendente dal fornitore, con operazioni idempotenti;
- revisioni, gestione dei conflitti senza perdite silenziose, tombstone;
- allegati con hash;
- nessun «vince l'ultimo timestamp» come unica regola;
- mai copiare il database SQLite e chiamarlo sincronizzazione.

L'utente non può o non vuole gestire account presso fornitori. Una cartella
già sincronizzata (Nextcloud, Syncthing, Dropbox, OneDrive…) non costa nulla
e non richiede server.

## Decisione

### Trasporto

Una cartella scelta dall'utente, sincronizzata da un servizio esterno. Al
suo interno Greatnote crea `greatnote-sync/`:

```text
greatnote-sync/
├── format.json                 {"format": 1, "library": "<uuid>"}
├── devices/<device-id>/
│   ├── device.json             nome e ultima attività del dispositivo
│   └── changes/000000000001.json …   lotti di modifiche, immutabili
└── blobs/<aa>/<sha256>         contenuto degli allegati, per hash
```

- **Un solo scrittore per cartella.** Ogni dispositivo scrive solo nella
  propria cartella e i lotti non vengono mai modificati: il servizio di
  sincronizzazione non può creare conflitti tra file.
- **Allegati per hash.** Due dispositivi che scrivono lo stesso allegato
  producono lo stesso file.
- **Scritture atomiche.** Ogni file viene scritto in un temporaneo e poi
  rinominato. Si leggono solo i nomi attesi, quindi un file a metà
  trasferimento viene ignorato fino al giro successivo.
- **Protocollo separato dal trasporto.** Un server futuro potrà
  trasportare gli stessi lotti.

### Modifiche locali

- **Registrazione automatica.** Trigger SQLite (migrazione 14) annotano in
  `sync_dirty` ogni entità creata, modificata o eliminata, qualunque sia il
  percorso del codice. Durante l'applicazione di modifiche remote un flag
  (`sync_control.applying`) li disattiva, così le modifiche non rimbalzano.
- **Entità sincronizzate:**
  - spazi, taccuini, tag, note, tag delle note, collegamenti tra note;
  - allegati (metadati, testo letto dalle immagini, contenuto in `blobs/`);
  - attività, eventi locali, promemoria, modelli, relazioni del cestino.
- **Restano locali:**
  - eventi di calendari esterni o iCal, e i loro promemoria;
  - bozze, cronologia delle versioni, visite («Recenti»);
  - indice AI, cronologia AI, impostazioni.
- **Esportazione a stato completo.** Ogni operazione contiene la riga
  intera (upsert) oppure un tombstone (eliminazione definitiva). Le
  modifiche ripetute alla stessa entità tra due sincronizzazioni diventano
  una sola operazione. Riapplicare un'operazione non cambia nulla.

### Versioni e ordine causale

- **Orologio di Lamport** per dispositivo. Ogni operazione ha `version =
  (contatore, dispositivo)` e `base` = la versione dell'entità che il
  dispositivo aveva quando l'ha modificata.
- **Dipendenze.** Ogni lotto ha un numero di sequenza per dispositivo e
  `deps`, cioè per ogni altro dispositivo l'ultimo lotto già applicato.
- **Consegna causale.** Un lotto viene applicato solo dopo il precedente
  dello stesso dispositivo e dopo tutte le sue dipendenze. Se il servizio
  di sincronizzazione consegna i file in disordine, il lotto aspetta il
  giro successivo.

### Conflitti

Con la consegna causale, un'operazione remota con `base` diversa dalla
versione locale è davvero concorrente.

- **Avanzamento:** se `base` coincide con la versione locale (o l'entità
  non esiste), si applica.
- **Concorrenza:** vince la versione maggiore `(contatore, dispositivo)`.
  Tutti i dispositivi scelgono lo stesso vincitore, quindi convergono. Se
  una delle due versioni è un'eliminazione, vince la modifica: nessun
  contenuto sparisce per un'eliminazione contemporanea.
- **Conservazione del perdente.** Per note e modelli la versione perdente
  viene salvata: per le note anche nella cronologia della nota, così si
  confronta e si ripristina dal dialog esistente. Viene registrata in
  `sync_conflicts`, visibile nelle impostazioni. Per le altre entità
  (nome di un tag, stato di un'attività…) vale la versione vincente; è
  documentato.
- **Nota aperta nell'editor.** Il salvataggio passa la versione su cui
  l'editor lavorava. Se nel frattempo è arrivata una modifica remota al
  testo, quella versione viene salvata nella cronologia e segnalata come
  conflitto prima di applicare il testo dell'editor. Senza modifiche locali
  in sospeso, l'editor ricarica la nota aggiornata.

### Casi particolari

- **Tag con lo stesso nome** creati su due dispositivi: vengono uniti con
  una tabella di alias (`sync_aliases`).
- **Riferimenti a entità eliminate definitivamente:** seguono le regole dello
  schema. Un taccuino eliminato lascia la nota senza taccuino; un tag della
  nota o un allegato di una nota eliminata vengono scartati.
- **Allegati.** I metadati arrivano con l'operazione; il file viene copiato
  da `blobs/` verificando lo SHA-256. Se il blob non è ancora arrivato
  l'allegato resta in attesa (`sync_missing_blobs`) e si riprova a ogni
  giro.

### Identità e sicurezza

- **Identità del dispositivo.** È generata alla prima attivazione e salvata
  nel database. Se nella propria cartella compaiono lotti che il database
  non conosce (lo stesso database copiato su due computer), la
  sincronizzazione si ferma con un errore invece di mescolare due storie.
- **Prima attivazione.** Tutte le entità esistenti vengono esportate.
  Unendosi a una cartella già usata, le note dei due dispositivi vengono
  unite.

### Esecuzione

- Ogni 2 minuti, all'avvio e su richiesta, solo se attiva.
- Esportazione e importazione avvengono con il database bloccato in
  un'unica sequenza. Ogni lotto viene importato in una transazione: un
  lotto illeggibile o non valido viene saltato e segnalato, senza bloccare
  gli altri.

## Alternative scartate

- **Copiare il file SQLite:** due dispositivi lo modificano e uno vince
  l'intero file. È vietato dalla specifica.
- **Ultimo timestamp:** gli orologi dei computer differiscono e le
  modifiche contemporanee andrebbero perse in silenzio.
- **CRDT sul contenuto delle note** (fusione carattere per carattere):
  molto più complesso. Oggi il perdente è conservato; si può aggiungere in
  futuro sulla stessa struttura di versioni.
- **Server proprio:** richiede hosting e autenticazione dei dispositivi.
  Resta possibile in futuro con lo stesso formato dei lotti.

## Conseguenze e limiti

- **Nessuna cifratura dei file.** Il servizio che ospita la cartella (per
  esempio Dropbox) può leggerli. Per la riservatezza: Syncthing tra
  dispositivi propri o Nextcloud personale. La cifratura con passphrase è
  in roadmap.
- **La cronologia dei lotti cresce.** Non c'è ancora una compattazione con
  istantanee: un nuovo dispositivo rilegge tutti i lotti.
- **Cartella indispensabile.** La sincronizzazione funziona solo se il
  servizio della cartella è attivo. Greatnote mostra l'ultimo esito e i
  dispositivi visti.
