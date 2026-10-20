# ADR 0007: attività, promemoria e calendario locale

- **Stato:** accettata
- **Data:** 2026-10-14

## Contesto

La fase 4 introduce attività con scadenza, promemoria con notifiche e un
calendario di eventi locali. Questi dati dipendono da date, fusi orari e ora
legale, e i promemoria devono scattare in modo affidabile anche dopo un
riavvio. Le integrazioni con calendari esterni restano fuori ambito (fase 6).

## Decisione

### Modello del tempo

- Libreria **`jiff`** 0.2 (MIT/Unlicense, BurntSushi): aritmetica di
  calendario consapevole del fuso, database IANA del sistema su Linux e
  incluso nel binario su Windows (`tzdb-bundle-platform`).
- **Istanti** (promemoria, eventi con orario): UTC `AAAA-MM-GGTHH:MM:SSZ`,
  ordinabili come testo in SQLite.
- **Date civili** (scadenze delle attività, eventi di un giorno intero):
  `AAAA-MM-GG` senza fuso. "Scade il 14 ottobre" resta vero anche se l'utente
  cambia fuso; la vista "Oggi" dipende solo dalla data locale corrente.
- Promemoria ed eventi salvano anche il **nome IANA del fuso** con cui sono
  stati creati, e il promemoria l'orario locale effettivo: servono per
  mostrare l'orario e per rinviare "a domani alla stessa ora".
- **Ora legale**: un orario inesistente (02:30 del giorno in cui si passa
  all'ora legale) viene spostato in avanti (03:30); un orario ripetuto
  (02:30 del ritorno all'ora solare) usa la prima occorrenza. È la strategia
  "compatible" di RFC 5545 e Temporal. Il rinvio "Domani" conserva l'orario
  locale anche se nel frattempo cambia l'ora legale.
- Le funzioni del core ricevono "oggi" o "adesso" come parametri: i test
  coprono fusi diversi (Roma, New York) e i giorni di cambio d'ora senza
  dipendere dall'orologio della macchina.

### Dati (migrazione v6)

- `tasks`: stato `open`/`done` con vincolo di coerenza su `completed_at`,
  priorità 0–3, scadenza civile validata da SQLite, nota facoltativa
  (`ON DELETE SET NULL`). Eliminazione definitiva con conferma: non c'è un
  cestino delle attività.
- `events`: con orario (istanti UTC) o di un giorno intero (date civili, fine
  inclusa), vincolo `CHECK` che impedisce combinazioni miste; colonne
  `calendar_provider`/`external_id` riservate e sempre `NULL`.
- `reminders`: polimorfici su nota, attività ed evento; stato `pending` →
  `fired` → `done`. I promemoria di attività ed eventi vengono eliminati da
  trigger insieme all'entità. Quelli di una nota nel cestino non scattano
  finché la nota non viene ripristinata.
- Una sola fonte di verità per i promemoria: la colonna `tasks.reminder_at`
  della specifica è sostituita dalla tabella `reminders` (più promemoria per
  attività; il prossimo è esposto come `next_reminder_at`).
- Completare un'attività chiude i suoi promemoria nella stessa transazione.
- Il filtro di ricerca `is:task` (alias `has:task`) seleziona le note con
  almeno un'attività aperta collegata.
- L'archivio completo `gna-archive` v1 include attività, eventi e promemoria
  non gestiti come **campi facoltativi** del manifest: gli archivi precedenti
  restano validi. In importazione i promemoria già passati vengono segnati
  come scattati, per non generare notifiche arretrate.

### Scheduler e notifiche

- Un thread della shell controlla ogni 15 secondi i promemoria con
  `remind_at <= adesso`, li segna come `fired` in un'unica transazione (ogni
  promemoria scatta una sola volta), invia una **notifica di sistema** con
  `tauri-plugin-notification` (D-Bus/libnotify su Linux, toast su Windows) e
  avvisa la UI con l'evento `reminders-fired`.
- I promemoria **persi** mentre l'app era chiusa o il computer sospeso
  scattano al primo controllo successivo, con il testo "Scaduto alle …:
  perso mentre l'app era chiusa" (ritardo oltre 1 minuto, dato che con l'app
  aperta il controllo avviene ogni 15 secondi). Al primo controllo dopo
  l'avvio una notifica riepiloga anche i promemoria scattati in sessioni
  precedenti e non ancora gestiti.
- Su Linux/BSD la notifica è inviata direttamente con `notify-rust` (la
  libreria che il plugin usa internamente, nessuna dipendenza nuova) in modo
  sincrono: il plugin la invia su un task separato e ne scarta l'esito, mentre
  qui gli errori di consegna finiscono nei log. Le notifiche non scadono e
  restano finché l'utente non le chiude. Su Windows e macOS resta il plugin. Il confronto avviene sull'istante UTC: cambi di fuso o di ora
  legale non fanno scattare né saltare promemoria.
- Nell'app i promemoria scattati restano in un riquadro persistente finché
  l'utente non li gestisce (apri, +10 min, +1 ora, domani, fatto, elimina): un
  errore nella notifica di sistema non fa perdere il promemoria.
- **Nessun servizio in background** quando l'app è chiusa: richiederebbe un
  demone o un'attività pianificata del sistema (systemd user timer, Utilità di
  pianificazione di Windows) con un proprio modello di permessi. Il limite è
  indicato nella UI e nella guida.
- Il frontend non riceve permessi del plugin di notifica: le notifiche
  partono solo dal processo Rust.

### UI

- WebKitGTK non offre un selettore per `<input type="datetime-local">`
  (l'ora non è sceglibile): gli orari usano un campo data nativo più due
  elenchi per ore e minuti (passo di 5 minuti).

- Barra laterale: **Attività** (contatore di oggi + in ritardo),
  **Promemoria** (contatore dei promemoria da gestire), **Calendario**.
- Attività con viste Oggi, In ritardo, Prossime, Senza scadenza, Tutte
  aperte, Completate; ricerca testuale parametrizzata.
- Calendario mensile e settimanale (elenco per giorno, non griglia oraria),
  eventi e scadenze delle attività; gli orari sono mostrati nel fuso del
  sistema, indicato in testata.
- Nella nota aperta: attività collegate e promemoria.

## Conseguenze

- Nuove dipendenze: `jiff` nel core, `tauri-plugin-notification` (MIT/
  Apache-2.0) nella shell.
- La ricerca testuale nelle attività usa `lower()` di SQLite: è insensibile
  alle maiuscole solo per i caratteri ASCII.
- Attività ricorrenti, promemoria ricorrenti, assegnazione ad altri utenti,
  azioni dalle notifiche e sincronizzazione con calendari esterni non sono
  implementati.
