# ADR 0034: fusione delle modifiche contemporanee a una nota

- **Stato:** accettata
- **Data:** 2026-11-26

## Contesto

Con la sincronizzazione (ADR 0017), due modifiche contemporanee alla stessa
nota vengono risolte così:
- una versione vince, scelta in modo deterministico;
- l'altra va nella cronologia della nota e tra le «Modifiche
  contemporanee».

Non si perde nulla, ma l'utente deve ricomporre a mano il testo anche quando
i due dispositivi hanno toccato parti diverse: per esempio un paragrafo
aggiunto in fondo su uno e un nome corretto in cima sull'altro.

## Decisione

### Fusione a tre vie per blocchi

- **Confronto per blocchi.** Si confrontano i blocchi del documento
  (paragrafi, titoli, elenchi, tabelle…) della versione di partenza comune
  con quelli delle due versioni, con la sottosequenza comune più lunga. Il
  modulo è `sync/merge.rs`.
- **Quando si uniscono.** Le modifiche si uniscono se toccano blocchi
  diversi. Se toccano lo stesso blocco con figli (elenco, citazione, tabella,
  elenco di attività), il confronto scende nei figli: due voci diverse dello
  stesso elenco si uniscono. Attributi e titolo si uniscono se li ha cambiati
  una parte sola o se sono cambiati allo stesso modo.
- **Quando non si uniscono:**
  - modifiche allo stesso paragrafo;
  - inserimenti diversi nello stesso punto;
  - un'aggiunta accanto a un blocco modificato dall'altra parte (l'ordine
    sarebbe ambiguo);
  - un blocco eliminato da una parte e modificato dall'altra;
  - titoli cambiati in modo diverso.

  In questi casi resta la regola di sempre.
- **Risultato deterministico:** dipende solo dai tre documenti e da quale
  versione vince.

### La partenza comune

- **Testo conservato.** Il testo e la base delle ultime 8 versioni
  sincronizzate di ogni nota (esportate o ricevute) stanno in
  `sync_note_bases` (migrazione 19). Sono dati del protocollo: vengono
  azzerati insieme alla sua storia.
- **Antenato verificato.** La partenza comune è la versione da cui è partita
  la modifica remota. Si fonde solo se quella versione è conservata ed è
  davvero un antenato della versione locale, seguendo le basi conservate.
  Altrimenti, per esempio subito dopo l'aggiornamento dalla 0.2.15, vale la
  regola di sempre.

### La fusione come modifica normale

- **Niente versioni speciali.** Il dispositivo che trova modifiche
  contemporanee non sovrapposte applica prima la regola di sempre (versione
  vincente). Poi salva il testo fuso come **propria modifica**, partita dalla
  versione vincente, che viene esportata subito nello stesso giro.
- **Convergenza.** Gli altri dispositivi ricevono la fusione con le regole
  esistenti. Se due dispositivi fondono le stesse versioni ottengono lo
  stesso testo e le due modifiche identiche non creano conflitti. La
  convergenza resta quella garantita dal protocollo.
- **Fusione non ancora esportata.** Una nota fusa in questo giro non
  corrisponde più alla sua versione: una modifica remota partita da quella
  versione viene trattata come contemporanea (quindi a sua volta fusa), non
  come un semplice avanzamento. La prova a tre dispositivi ha mostrato che
  altrimenti una modifica andava persa.
- **Nessuna perdita.** Il testo locale di prima della fusione va nella
  cronologia della nota. Un editor aperto su quella nota non la sovrascrive
  in silenzio: vale lo stesso meccanismo delle modifiche remote.

## Alternative scartate

- **CRDT o trasformazione operazionale sul testo:** fusione carattere per
  carattere, ma richiede di cambiare il formato dei lotti e di conservare
  tutte le operazioni. È sproporzionato per un'app di note.
- **Fusione dentro i paragrafi** (per parole): rischia frasi incoerenti
  senza che l'utente se ne accorga. Lo stesso paragrafo cambiato due volte
  resta un conflitto da controllare.
- **Versioni sintetiche per il testo fuso**, calcolate da ogni dispositivo:
  due dispositivi potrebbero arrivare a versioni diverse con lo stesso
  testo, e una modifica partita dalla versione vincente prima della fusione
  cancellerebbe le modifiche dell'altra parte.

## Conseguenze

- Le modifiche contemporanee a parti diverse di una nota non richiedono più
  interventi e non compaiono tra le «Modifiche contemporanee».
- Una fusione costa una modifica in più nella cartella. Raramente ne
  arrivano due uguali, da dispositivi diversi.
- Prove:
  - test della fusione pura;
  - prove su due e tre dispositivi;
  - ripiego senza base;
  - prova casuale a tre dispositivi verificata su 40 semi
    (`GREATNOTE_MERGE_SEED`).
