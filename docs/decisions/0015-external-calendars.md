# ADR 0015: calendari esterni (Google Calendar e Microsoft Outlook/365)

- **Stato:** accettata
- **Data:** 2026-10-20

## Contesto

L'utente vuole vedere e gestire nel calendario di Greatnote gli eventi di
Google Calendar e di Outlook/Microsoft 365. La specifica chiede OAuth con
permessi minimi, token mai in chiaro, revoca, distinzione fra eventi locali
ed esterni e nessuna sincronizzazione bidirezionale senza una regola per i
conflitti. L'integrazione non va dichiarata operativa finché OAuth, API ed
errori non funzionano.

## Decisione

- **Registrazione dell'editore, incorporata nella build.**
  - Come nelle altre app, l'editore registra Greatnote una volta presso
    Google e Microsoft (docs/OAUTH_SETUP.md). Gli ID entrano nel programma
    al momento della compilazione (`option_env!` su
    `GREATNOTE_GOOGLE_CLIENT_ID`, `GREATNOTE_GOOGLE_CLIENT_SECRET`,
    `GREATNOTE_MICROSOFT_CLIENT_ID`) e non stanno nel repository.
  - L'utente preme «Collega», accede e consente.
  - Per le app native gli ID non sono segreti: Google dichiara non
    confidenziale il segreto dei client desktop, e Microsoft usa un client
    pubblico senza segreto con PKCE.
  - **Registrazione personale facoltativa:** nelle impostazioni avanzate
    l'utente può inserire i propri ID, che hanno la precedenza (segreto
    Google nel portachiavi). Una build senza ID incorporati richiede questa
    strada.
- **OAuth per app native (RFC 8252).**
  - Codice di autorizzazione con PKCE S256, `state` casuale, browser di
    sistema.
  - Ritorno su loopback con porta casuale: `http://127.0.0.1:<porta>` per
    Google; per Microsoft `http://localhost:<porta>`, ascoltando IPv4 e IPv6
    sulla stessa porta.
  - Le richieste con `state` diverso vengono ignorate. L'accesso scade dopo
    5 minuti e si può annullare.
- **Permessi minimi.**
  - Google: `calendar.events` e `calendar.calendarlist.readonly`.
  - Microsoft: `Calendars.ReadWrite`, `User.Read` (indirizzo dell'account)
    e `offline_access`.
- **Token.** Il token di aggiornamento va nel portachiavi
  (`greatnote-calendar`, `account-<id>`) e viene sostituito quando il
  fornitore lo ruota. Il token di accesso resta solo in memoria. Il
  database contiene solo account, calendari ed eventi.
- **Il fornitore è la fonte di verità.** Non è una sincronizzazione
  bidirezionale con fusione:
  - Greatnote tiene una copia locale degli eventi dei calendari selezionati,
    da 90 giorni prima a 400 giorni dopo oggi, nella tabella `events`
    (colonne `external_calendar_id`, `external_id`, `etag`, `web_link`,
    `read_only`), così il calendario funziona anche offline.
  - A ogni sincronizzazione la copia di ogni calendario viene allineata:
    aggiornati i presenti, rimossi gli assenti.
  - Nota collegata e promemoria locali dell'evento vengono conservati.
- **Modifiche dirette e condizionate.**
  - Creare, modificare ed eliminare un evento esterno chiama subito l'API
    del fornitore, con `If-Match` sull'ETag.
  - Se l'evento è cambiato altrove l'operazione fallisce (412) con
    l'invito a sincronizzare: nessuna sovrascrittura silenziosa.
  - Il database rifiuta modifiche locali e il cestino per gli eventi
    esterni (`StorageError::ExternalEvent`), così nessun percorso li
    altera senza il fornitore.
  - L'eliminazione è definitiva presso il fornitore, previa conferma che lo
    dice esplicitamente.
- **Ricorrenze in sola lettura.** Le ricorrenze arrivano espanse in singole
  occorrenze (`singleEvents=true`, `calendarView`) e restano in sola
  lettura, come gli eventi dei calendari senza permesso di scrittura. Per
  modificarle si apre l'evento nel calendario d'origine (solo link
  `https://`).
- **Sincronizzazione.** In background ogni 15 minuti (la prima dopo
  20 secondi), più manuale da calendario e impostazioni. Gli errori restano
  visibili per account; un token revocato chiede di ricollegare.
- **Scollegamento.**
  - Google: revoca del token presso il fornitore.
  - Microsoft: l'API non permette di revocare un singolo token, quindi la
    guida spiega come togliere l'accesso dall'account.
  - In entrambi i casi il token viene cancellato dal portachiavi e la
    copia locale viene rimossa con i suoi promemoria.
- **Archivi.** Gli eventi esterni e i loro promemoria non entrano
  nell'esportazione completa: appartengono al fornitore e verrebbero
  duplicati importandoli.
- **HTTP.**
  - Client `ureq` già in uso, senza reindirizzamenti.
  - La paginazione di Graph (`@odata.nextLink`) è seguita solo sullo
    stesso server.
  - Risposte limitate a 16 MB; errori accorciati e senza token.

## Alternative scartate

- **ID client scritti nel codice sorgente:** il repository è pubblico e la
  regola del progetto vieta credenziali nel codice. Con le variabili di
  compilazione ogni build sceglie le proprie registrazioni.
- **Flusso "device code":** più scomodo e non disponibile per tutti gli
  account Google.
- **Sincronizzazione incrementale (`syncToken`, delta query):** rimandata.
  La finestra completa è semplice, idempotente e sufficiente per calendari
  personali.
- **Fusione automatica delle modifiche concorrenti:** contraria alla regola
  di non perdere contenuti senza confronto.

## Conseguenze

- **Stato al 2026-10-20: sospesa.** L'editore non può registrare le app
  presso Google e Microsoft. La sezione delle impostazioni è nascosta se la
  build non include ID e non esistono account o registrazioni personali. Il
  codice resta, coperto dai test con server finti, senza alcuna prova reale.

- Finché l'app Google dell'editore è in modalità "Test", l'accesso scade
  dopo 7 giorni ed è limitato agli utenti di test. Pubblicata ma non
  verificata, mostra un avviso ed è limitata a 100 utenti. La verifica di
  Google (ambito sensibile `calendar.events`) toglie entrambi i limiti.
- La creazione di occorrenze o serie ricorrenti da Greatnote non è
  supportata.
