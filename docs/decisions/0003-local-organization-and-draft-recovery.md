# ADR 0003: organizzazione locale e recupero delle bozze

- **Stato:** accettata
- **Data:** 2026-10-14

## Contesto

Note, spazi, taccuini e tag devono restare utilizzabili offline. Le modifiche
non applicate non devono sovrascrivere in modo silenzioso una versione
persistita, e rimuovere un contenitore organizzativo non deve cancellare note.

## Decisione

Conservare l'organizzazione in SQLite: ogni nota appartiene al massimo a un
taccuino e può avere più tag; ogni taccuino può appartenere a uno spazio.
Eliminare uno spazio scollega i suoi taccuini, eliminare un taccuino scollega
le note, ed eliminare un tag cancella le associazioni in transazione. Le note
sono spostate nel cestino con cancellazione logica e possono essere
ripristinate.

Scrivere le bozze in una tabella locale prima di applicarle alla nota. Al
successivo accesso alla nota, chiedere all'utente se ripristinarle o scartarle.
Aggiornamento della nota e rimozione della bozza applicata avvengono nella
stessa transazione SQLite.

## Conseguenze

- L'eliminazione di contenitori non rimuove contenuti dell'utente.
- La migrazione v2 aggiunge la tabella bozze senza ricreare o sostituire dati
  esistenti nello schema v1.
- Le bozze sono conservate in chiaro nello stesso database locale delle note;
  cifratura e backup restano fuori dallo scope attuale.
- Lo svuotamento definitivo del cestino e la cronologia revisioni sono
  funzionalità successive.
