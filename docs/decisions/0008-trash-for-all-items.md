# ADR 0008: cestino per spazi, taccuini, tag, attività ed eventi

- **Stato:** accettata
- **Data:** 2026-10-14

## Contesto

Finora solo le note passavano dal cestino. Eliminare uno spazio, un
taccuino, un tag, un'attività o un evento era immediato e definitivo, e per
spazi e taccuini scollegava i contenuti senza possibilità di ripristino. Si
vuole che ogni elemento eliminabile finisca nel cestino, sia ripristinabile e
venga eliminato definitivamente solo su richiesta (singolarmente o svuotando
il cestino).

## Decisione

- **Migrazione v7:** `deleted_at` su `tags`, `tasks`, `events` (spazi e
  taccuini avevano già la colonna, inutilizzata) e tabella
  `trash_relations (item_type, item_id, related_id)`.
- **Collegamenti staccati e registrati.** Quando un contenitore va nel
  cestino i suoi collegamenti vengono rimossi e annotati in `trash_relations`:
  - taccuino → le note diventano "senza taccuino";
  - spazio → i taccuini restano senza spazio;
  - tag → il tag viene tolto dalle note.

  In questo modo ricerca, viste, filtri ed esportazioni non vedono mai
  riferimenti a elementi nel cestino senza dover cambiare ogni query, e
  **nessuna nota viene nascosta o eliminata** come effetto collaterale.
  Alternativa scartata: lasciare i collegamenti e filtrare ovunque. Avrebbe
  richiesto condizioni aggiuntive in decine di query, con il rischio di note
  invisibili se una query fosse rimasta indietro.
- **Ripristino:** l'elemento torna attivo e i collegamenti registrati vengono
  ricreati solo se l'elemento collegato esiste ancora e non è stato
  riassegnato nel frattempo (una nota spostata in un altro taccuino resta
  dove l'utente l'ha messa).
- **Attività ed eventi** nel cestino sono esclusi da viste, contatori,
  calendario, filtro `is:task` ed esportazione; i loro promemoria non
  scattano finché non vengono ripristinati.
- **Eliminazione definitiva** solo per elementi già nel cestino. Spazi e
  taccuini eliminati lasciano i contenuti senza contenitore (`ON DELETE SET
  NULL`); i promemoria di attività ed eventi spariscono con i trigger.
  **Svuota cestino** elimina note (con i file allegati) ed elementi.
- **Validazioni:** non si possono creare note in un taccuino nel cestino,
  spostarvi note, assegnare taccuini a uno spazio nel cestino o assegnare tag
  nel cestino. I nomi dei tag restano unici anche rispetto al cestino: creare
  un tag con il nome di uno cestinato restituisce un errore che invita a
  ripristinarlo o eliminarlo.
- **Importazioni:** se un file o un archivio importato fa riferimento a un
  tag, taccuino o spazio nel cestino, l'elemento viene ripristinato, così le
  note importate non finiscono collegate a un elemento invisibile.
- **UI:** nella vista Cestino, sopra le note, la sezione "Altri elementi"
  elenca gli elementi con tipo, data, collegamenti da ricreare e i pulsanti
  Ripristina ed Elimina definitivamente (con conferma).

## Conseguenze

- I comandi `delete_space`, `delete_notebook`, `delete_tag`, `delete_task` e
  `delete_event` sono sostituiti da `move_item_to_trash`,
  `restore_trash_item` e `delete_trash_item`.
- Gli allegati rimossi da una nota e i promemoria non passano dal cestino:
  i promemoria gestiti restano nello storico della sezione Promemoria.
- Il cestino non è incluso nell'archivio completo né nell'esportazione
  Markdown; è incluso nei backup (che copiano l'intero database).
