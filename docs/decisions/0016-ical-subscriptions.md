# ADR 0016: abbonamenti a calendari iCal in sola lettura

- **Stato:** accettata
- **Data:** 2026-10-20

## Contesto

L'integrazione con le API di Google e Microsoft (ADR 0015) è sospesa:
l'utente non può registrare le app presso i fornitori. Inoltre la modalità
Test di Google richiede di ricollegarsi ogni 7 giorni. L'utente vuole
comunque vedere i propri appuntamenti nel calendario di Greatnote.

## Decisione

- **Indirizzo iCal (RFC 5545).**
  - L'utente incolla l'indirizzo iCal del calendario: per Google
    l'«indirizzo segreto in formato iCal», per Outlook il collegamento ICS
    di un calendario pubblicato.
  - Nessun login, nessuna registrazione, nessun token che scade.
- **Sola lettura.** Gli eventi si modificano nel calendario d'origine.
  - Nel database la colonna `subscription_id` (migrazione 13) e
    `calendar_provider = 'ics'` bloccano modifiche locali e cestino
    (`StorageError::ExternalEvent`).
  - Restano possibili promemoria e collegamento a una nota.
- **Indirizzo nel portachiavi.** Chi conosce l'indirizzo vede il
  calendario, quindi lo trattiamo come un segreto: resta nel portachiavi
  (`greatnote-calendar`, `ics-<id>`), mai nel database, nei log o nei
  messaggi.
- **Rete.**
  - Solo `https://` (anche `webcal://`, convertito in https).
  - Niente credenziali nell'indirizzo e nessun reindirizzamento seguito.
  - Al massimo 16 MB; `http://` solo verso il computer stesso, per i test.
  - Un indirizzo che non restituisce un calendario non viene salvato.
- **Lettura propria** (`src/calendar_sync/ics.rs`), senza nuove dipendenze
  (usa `jiff`, già presente).
  - **Testo:** righe ripiegate e sequenze di escape.
  - **Fusi orari:** IANA; alias `X-LIC-LOCATION`; nomi Windows di Outlook
    tramite una tabella dei più diffusi; altrimenti fuso del sistema.
  - **Eventi:** giorni interi, `DURATION`, `STATUS:CANCELLED`, `EXDATE`,
    `RECURRENCE-ID`.
  - **Ricorrenze:** `RRULE` con `FREQ` giornaliera, settimanale, mensile o
    annuale e `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY` (anche con ordinali
    mensili), `BYMONTHDAY`, `BYMONTH`, `BYSETPOS`.
  - **Orari:** le occorrenze sono calcolate nell'ora locale del fuso
    dell'evento, quindi restano corrette al cambio dell'ora legale.
  - **Regole non supportate** (`BYHOUR`, `BYWEEKNO`, frequenze orarie…):
    viene mostrata solo la prima occorrenza, invece di date sbagliate.
  - **Limiti:** 20.000 occorrenze e 50.000 periodi per regola; le regole
    lunghe senza `COUNT` saltano direttamente alla finestra.
- **Aggiornamento.**
  - Finestra di 90 giorni indietro e 400 avanti, come per gli account.
  - Ogni 15 minuti e su richiesta, insieme agli account.
  - Ogni occorrenza ha un id stabile (`UID@inizio`). Gli eventi spariti
    vengono rimossi con i loro promemoria; nota collegata e promemoria
    degli altri restano.
- **Esportazione:** gli eventi degli abbonamenti, come quelli esterni,
  restano fuori dall'archivio dell'app.

## Alternative scartate

- **Crate `icalendar`/`rrule`:** il secondo dipende da `chrono`/`chrono-tz`
  e non gestisce i nomi di fuso di Windows. Il sottoinsieme necessario è
  limitato e coperto da test.
- **Indirizzo salvato nel database:** finirebbe nei backup e nelle copie del
  database; il portachiavi lo protegge meglio.

## Conseguenze

- I calendari aziendali Microsoft 365 possono avere la pubblicazione
  disattivata dall'amministratore.
- Google aggiorna l'indirizzo iCal con qualche ritardo (anche ore): gli
  eventi appena creati possono comparire più tardi.
- I partecipanti e le risposte agli inviti non vengono mostrati.
