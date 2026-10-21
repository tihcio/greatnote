# ADR 0022: Attività, calendario e promemoria come contesto della chat AI

- **Stato:** accettata
- **Data:** 2026-10-21

## Contesto

La chat dell'assistente (ADR 0010) recupera solo frammenti di note. Domande
come «Cosa ho domani?» o «Quando devo rinnovare il passaporto?» riguardano
invece attività, eventi del calendario (locali ed esterni) e promemoria, che
stanno in tabelle proprie e non nell'indice delle note. L'utente vuole
cercarli dall'assistente e usarli come contesto.

Vincoli già in vigore per le note, da mantenere:

- fonti verificabili, citate con `[n]` e apribili dall'interfaccia;
- testo riletto dal database al momento dell'invio (il frontend passa solo
  riferimenti);
- taccuini esclusi mai inviati;
- anteprima e conferma per i server remoti;
- nessuna query costruita dal testo dell'utente.

## Decisione

- **Le fonti hanno un tipo:** `note`, `task`, `event`, `reminder`.
  `SourceRef` aggiunge `kind` e `item_id` (con valori predefiniti, così i
  riferimenti precedenti restano validi); `ContextSource` aggiunge `open`,
  l'elemento da aprire. Un promemoria di un'attività o di un evento confluisce
  nella fonte di quell'elemento; quello di una nota è una fonte a sé che apre
  la nota.
- **Analisi locale e deterministica della domanda** (`ai::planner_context`),
  in italiano e inglese:
  - espressioni di tempo → intervallo di giorni civili nel fuso del sistema
    (oggi, domani, ieri, settimane, fine settimana, mesi, giorni della
    settimana, date numeriche e scritte, «prossimi N giorni»; al massimo 92
    giorni);
  - parole del planner («attività», «calendario», «promemoria»,
    «scadenze»…) senza date → prossimi 14 giorni, attività in ritardo e senza
    scadenza, promemoria scattati;
  - le parole restanti si cercano in titolo e descrizione di attività ed
    eventi, a qualunque data, con query parametrizzate.

  Si è scelto di non chiedere al modello di estrarre le date: servirebbe una
  richiesta in più (lenta con i modelli locali) e il testo dell'utente
  potrebbe pilotare la ricerca.
- **Budget:** gli elementi del planner usano al massimo metà di
  `max_context_chars` (12 elementi, descrizioni accorciate a 400 caratteri);
  le note usano il resto. Se la domanda nomina un periodo, gli elementi del
  periodo vengono prima delle note.
- **Data corrente nel prompt:** la domanda inviata al modello inizia con
  data, ora e fuso, così «domani» ha un riferimento. Le istruzioni di sistema
  descrivono i quattro tipi di fonte, che restano dati non attendibili.
- **Privacy:** impostazione `planner_context`, attiva per impostazione
  predefinita perché gli elementi passano dalla stessa anteprima delle note.
  Gli elementi collegati a note di taccuini esclusi non vengono mai inviati.
  Gli eventi dei calendari esterni sono inclusi: sono dati dell'utente già
  copiati in locale.
- **Cronologia:** le fonti salvate ricordano tipo, elemento e destinazione;
  le voci precedenti restano leggibili come note.

## Conseguenze

- Le domande sul calendario funzionano anche senza indice semantico e con
  qualunque provider.
- Il riconoscimento delle date copre le forme comuni ma non tutte (per
  esempio «tra tre settimane» o «il primo lunedì del mese»); in quei casi
  restano la ricerca per parole e il riepilogo dei prossimi 14 giorni.
- Il confronto senza maiuscole nella ricerca per parole vale per l'ASCII,
  come nella ricerca delle attività.
- L'assistente non crea né modifica attività ed eventi: le proposte restano
  quelle di «Estrai attività», con conferma.
