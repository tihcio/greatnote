# ADR 0018: Dropbox come trasporto della sincronizzazione

- **Stato:** accettata
- **Data:** 2026-10-20

## Contesto

L'utente vuole sincronizzare le note collegando l'account Dropbox, come in
Joplin, senza installare il client di Dropbox. La sincronizzazione tramite
cartella (ADR 0017) richiede invece un servizio che sincronizzi una cartella
locale.

## Decisione

- **Trasporto astratto.** Il protocollo di ADR 0017 (lotti, versioni,
  conflitti, blob) lavora sopra il trait `SyncStore`, che prevede quattro
  operazioni: leggere, scrivere (anche «solo se non esiste»), verificare
  l'esistenza, elencare. Lo implementano:
  - `LocalStore`, la cartella locale sincronizzata;
  - `DropboxStore`, che usa le API v2 di Dropbox (`files/download`,
    `files/upload` in modalità `add`/`overwrite`, `files/get_metadata`,
    `files/list_folder`).

  Il formato dei file è lo stesso nei due casi.
- **App registrata dall'editore.**
  - **Accesso:** limitato alla cartella dell'app (`Applicazioni/<nome
    app>`), con i soli permessi sui file e `account_info.read`.
  - **App key:** entra nella build da `GREATNOTE_DROPBOX_APP_KEY` (file
    `oauth.env` escluso da git, o segreti della CI). Non serve il segreto
    dell'app.
- **Accesso con OAuth 2 e PKCE (S256).**
  - Il login avviene nel browser di sistema; `state` casuale verificato.
  - Il ritorno arriva su `http://localhost:41927/`: Dropbox confronta
    l'indirizzo esattamente con quello registrato, quindi la porta è fissa.
  - Si ascolta su IPv4 e IPv6 solo durante l'accesso, al massimo 5 minuti,
    con possibilità di annullare.
  - `token_access_type=offline` fornisce un token di aggiornamento che non
    scade finché l'accesso non viene revocato. Lo conserviamo nel
    portachiavi (`greatnote-sync`, `dropbox-refresh`); il token di accesso
    resta solo in memoria.
- **Robustezza.** In caso di 429 o 5xx si riprova con attesa (al massimo
  30 s, 4 tentativi). Un 401 svuota il token in memoria; se anche
  l'aggiornamento fallisce, viene chiesto di ricollegare l'account.
- **Cambio di destinazione.** Passare da cartella a Dropbox, o cambiare
  account, è un'altra libreria: la storia locale del protocollo viene
  azzerata e le note vengono riesportate e unite (ADR 0017).
- **Scollegamento.** Il token viene revocato presso Dropbox, cancellato dal
  portachiavi e la sincronizzazione si disattiva. Le note restano su questo
  computer e su Dropbox.

## Alternative scartate

- **Client Dropbox con cartella locale:** funziona già con ADR 0017, ma
  richiede di installare il client.
- **Flusso senza redirect (codice da copiare e incollare):** più scomodo;
  il ritorno sul loopback è lo standard per le app desktop (RFC 8252).
- **SDK Dropbox:** non esiste un SDK Rust ufficiale. Le chiamate
  necessarie sono poche e coperte da test con un server finto.

## Conseguenze

- **Riservatezza:** Dropbox può leggere i file, che non sono cifrati. La
  cifratura con passphrase è in roadmap.
- **App in sviluppo:** finché l'app Dropbox dell'editore è in stato
  «development», Dropbox ne limita gli utenti; per distribuirla a tutti
  serve l'approvazione di Dropbox.
- **Porta occupata:** se un altro programma usa la porta 41927, il
  collegamento lo segnala.
