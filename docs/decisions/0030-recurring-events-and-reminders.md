# ADR 0030 — Eventi ricorrenti e promemoria che si ripetono

- Stato: accettata
- Data: 2026-11-25
- Collegata a: ADR 0026 (attività ricorrenti)

## Contesto

Le attività si ripetevano già (ADR 0026); eventi del calendario locale e
promemoria sulle note no. Un evento ricorrente deve comparire in tutte le
viste del calendario, nel contesto dell'AI e nella Home senza creare righe
per ogni occorrenza, e deve restare corretto con l'ora legale e con la
sincronizzazione tra dispositivi.

## Decisione

- **Stessa regola delle attività** (`domain::recurrence::Recurrence`, JSON)
  nella colonna `events.recurrence` (migrazione v16), con la data d'inizio
  dell'evento nel suo fuso come primo giorno della serie.
- **Occorrenze calcolate, non salvate.** `list_events` espande le serie
  nell'intervallo richiesto; ogni occorrenza ha l'id della serie e
  `occurrence_date`. Un evento con orario mantiene l'ora locale del primo
  nel fuso dell'evento e la stessa durata: con il cambio dell'ora l'istante
  UTC cambia, l'orario sul calendario no. Le occorrenze iniziate prima
  dell'intervallo che lo intersecano (eventi di più giorni) sono incluse.
- **Eccezioni** in `events.recurrence_exceptions` (array JSON di date):
  - «Solo questa» (eliminare): la data viene aggiunta alle eccezioni;
  - «Solo questa» (modificare): eccezione più un nuovo evento a sé, con id
    deterministico derivato da serie e data, così ripeterla non duplica;
  - «Questa e le successive»: `until` al giorno prima; dalla prima
    occorrenza l'intera serie va nel cestino;
  - «Tutta la serie» da un'occorrenza: lo spostamento in giorni e l'orario
    scelti si applicano dal primo giorno della serie.
  Togliendo la ripetizione si azzerano anche le eccezioni.
- **Promemoria delle note** con una regola propria (`reminders.recurrence`):
  quando uno scatta nasce il successivo alla stessa ora locale, dalla prima
  data utile da oggi (le occorrenze perse non si ricreano).
- **Promemoria di un evento ricorrente**: si ripetono con l'evento senza
  regola propria. Quando uno scatta, il successivo viene creato per
  l'occorrenza seguente (saltando le eccezioni) con lo stesso anticipo.
- **Attività**: nessuna regola sul promemoria; segue la ripetizione
  dell'attività (ADR 0026).
- **Id deterministici** per i promemoria successivi (come ADR 0026): due
  dispositivi che vedono scattare lo stesso promemoria creano la stessa riga.
- **Archivio e sincronizzazione**: le colonne nuove viaggiano nell'archivio
  completo (`recurrence`, `recurrence_exceptions` degli eventi,
  `recurrence` dei promemoria) e nella sincronizzazione; una versione
  precedente non le azzera (vedi ADR 0026).

## Alternative scartate

- **Una riga per occorrenza** fino a un orizzonte: moltiplica i dati
  sincronizzati, richiede un processo che allunghi la serie e rende costosa
  la modifica di tutta la serie.
- **RRULE/EXDATE di iCalendar come testo**: più espressivi, ma la regola
  JSON è già usata dalle attività e dall'interfaccia.

## Limiti

- Le eccezioni sono un campo della riga: se due dispositivi tolgono
  occorrenze diverse della stessa serie mentre sono scollegati, vince
  l'ultima modifica (le occorrenze restano visibili, nessun dato perso).
- Su Android il promemoria successivo nasce quando l'app elabora quello
  scattato: se l'app resta chiusa a lungo, la ripetizione riprende alla
  riapertura.
- Gli eventi esterni (Google, Microsoft, iCal) non si modificano come serie
  da Greatnote.
