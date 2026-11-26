# ADR 0033: istantanee della libreria per i dispositivi che entrano

- **Stato:** accettata
- **Data:** 2026-11-26

## Contesto

I lotti della sincronizzazione sono immutabili e non vengono mai eliminati
(ADR 0017): servono a chi entra nella libreria dopo. Un dispositivo nuovo
però li rilegge tutti, al massimo 100 per dispositivo a ogni giro (uno ogni
2 minuti). Con qualche migliaio di lotti l'ingresso richiederebbe ore e
molte chiamate a Dropbox.

## Decisione

- **Istantanea.** Ogni tanto un dispositivo scrive nella **propria**
  cartella (resta valida la regola di un solo scrittore per cartella)
  un'istantanea della libreria con:
  - le operazioni: per ogni entità sincronizzata con una versione, la riga
    attuale (o il tombstone, se eliminata) con la sua versione;
  - `covers`: fin dove arrivava la storia di ogni dispositivo, autore
    compreso;
  - il contatore di Lamport e i tag unificati per nome.

  ```text
  devices/<id>/snapshots/<lamport>.json          manifesto
  devices/<id>/snapshots/<lamport>/000001.json …  parti (2000 operazioni o 8 MB)
  ```

  Il manifesto viene scritto per ultimo: chi lo trova trova anche le parti.
  In una libreria cifrata (ADR 0032) manifesto e parti sono cifrati come
  gli altri file.
- **Quando scriverla.** Al massimo una volta al giorno, dopo una
  sincronizzazione riuscita, un dispositivo conta i lotti non coperti
  dall'istantanea più recente di ogni dispositivo. Se sono almeno 500, ne
  scrive una. Lo stato viene letto in una sola transazione e solo senza
  modifiche locali ancora da esportare: così l'istantanea corrisponde
  esattamente ai lotti che copre. Con 20.000 note si costruisce in circa
  150 ms (12 MB in 10 parti).
- **Pulizia.** Restano le ultime 2 istantanee del dispositivo, così chi sta
  leggendo la precedente non la perde a metà. Per questo `SyncStore` ha un
  metodo `remove`, usato solo qui. **I lotti non vengono mai eliminati.**
- **Ingresso.** Un dispositivo alla prima sincronizzazione nella libreria
  sceglie l'istantanea che copre più lotti (ne prova fino a 3, se una non
  si legge). Con il database bloccato:
  1. esporta le proprie note, come sempre;
  2. applica l'istantanea con le stesse regole dei lotti: versione per
     versione, conflitti decisi allo stesso modo, quelli tra altri
     dispositivi già risolti non segnalati;
  3. segna i dispositivi coperti come applicati fin lì;
  4. legge solo i lotti successivi.

  Il risultato è quello che otterrebbe rileggendo tutti i lotti coperti.
- **Errori.** Un'istantanea mancante, incompleta, di un'altra libreria o
  non valida viene ignorata, con un avviso, e si rileggono i lotti.
- **Compatibilità.** Le versioni precedenti non guardano la cartella
  `snapshots` e continuano a leggere i lotti. Il formato della libreria non
  cambia.

## Alternative scartate

- **Eliminare i lotti coperti:** un dispositivo offline da tempo (o una
  versione che non conosce le istantanee) non potrebbe più recuperare le
  modifiche. Lo spazio occupato dai lotti resta un limite noto.
- **Istantanea come file unico:** su archivi grandi supererebbe il limite
  di lettura (64 MB) e non si potrebbe scrivere a pezzi.
- **Leggere più lotti per giro:** riduce i giri ma non il numero di file da
  scaricare.

## Conseguenze

- Un nuovo dispositivo entra in pochi giri, indipendentemente dalla
  lunghezza della storia.
- Nella cartella c'è una copia completa in più della libreria (due per
  dispositivo che le scrive), senza allegati, che restano nei blob.
