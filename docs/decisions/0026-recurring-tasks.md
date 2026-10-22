# ADR 0026: attività ricorrenti

- **Stato:** accettata
- **Data:** 2026-10-22

## Contesto

La fase 4 prevedeva attività e promemoria ricorrenti (bollette, report
settimanali, scadenze annuali). Le attività si sincronizzano tra
dispositivi (ADR 0017) e i promemoria hanno orari locali con fuso (ADR
0007).

## Decisione

- **Regola** salvata nella colonna `tasks.recurrence` (migrazione v15) come
  JSON validato in Rust (`domain::recurrence::Recurrence`): sottoinsieme di
  RRULE con `frequency` (`daily`, `weekly`, `monthly`, `yearly`),
  `interval` 1–99, `weekdays` ISO per le settimanali, `month_day` per
  mensili e annuali, `until` facoltativo. Campi sconosciuti rifiutati. Serve
  una scadenza, da cui parte la serie.
- **Completamento:** l'attività resta completata (storico) e nella stessa
  transazione nasce la successiva, con gli stessi dati e la stessa regola,
  alla prima data della serie successiva alla scadenza e non precedente a
  oggi: le scadenze saltate non vengono ricreate. I promemoria vengono
  copiati spostati degli stessi giorni alla stessa ora locale nel loro fuso
  (quindi corretti anche attraverso l'ora legale), solo se nel futuro.
- **Id deterministico** della nuova occorrenza (UUID v8 da SHA-256 di id
  precedente e nuova data; lo stesso per i promemoria). Riaprire e
  ricompletare non duplica; due dispositivi che completano la stessa
  attività creano la stessa riga, che la sincronizzazione unisce.
- **Mese corto:** `month_day` conserva il giorno scelto; il 31 cade il 30 o
  il 28 e torna al 31 il mese dopo. Il 29 febbraio annuale cade il 28 negli
  anni non bisestili.
- **Sincronizzazione con versioni diverse:** una colonna assente
  nell'istantanea ricevuta non viene più azzerata (resta il valore locale;
  in inserimento vale il default). Un dispositivo non aggiornato quindi non
  cancella la regola; completando l'attività però non crea la successiva.

## Alternative scartate

- **Spostare la scadenza della stessa attività** (come alcune app): perde lo
  storico delle completate e, con la sincronizzazione, due completamenti
  concorrenti spostano la data due volte.
- **RRULE completo:** troppo per l'uso previsto e difficile da presentare
  nell'interfaccia; la regola JSON può crescere con campi nuovi (versioni
  precedenti la ignorerebbero come regola illeggibile, cioè senza
  ripetizione).

## Limiti

- Gli eventi del calendario locale e i promemoria sulle note non si
  ripetono ancora.
- Una regola che una versione precedente non sa leggere equivale a nessuna
  ripetizione su quella versione.
