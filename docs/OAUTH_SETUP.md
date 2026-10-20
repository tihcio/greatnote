# Registrare Greatnote presso Google e Microsoft (editore)

Questa guida è per chi pubblica Greatnote. Le app vanno registrate una sola
volta; i loro ID vengono incorporati nella build, così gli utenti premono
**Collega**, accedono al proprio account e consentono l'accesso, senza
configurare nulla.

Gli ID non vanno nel repository. Si passano come variabili d'ambiente al
momento della compilazione:

| Variabile | Contenuto |
|---|---|
| `GREATNOTE_GOOGLE_CLIENT_ID` | ID client Google di tipo «App desktop» |
| `GREATNOTE_GOOGLE_CLIENT_SECRET` | Segreto del client Google. Per le app desktop Google lo considera non confidenziale, ma resta fuori dal repository. |
| `GREATNOTE_MICROSOFT_CLIENT_ID` | ID applicazione (client) Microsoft, un GUID |

In locale si usa `oauth.env`, copiato da `oauth.env.example` ed escluso da
git:

```sh
cp oauth.env.example oauth.env   # poi compila i valori
set -a; source oauth.env; set +a
npm run tauri:dev                # oppure npm run tauri:build
```

In CI vanno messi tra i segreti del repository (es. GitHub Actions
`secrets.GREATNOTE_GOOGLE_CLIENT_ID`) ed esposti come variabili d'ambiente
del passo di build. Una build senza variabili funziona comunque: l'utente
può inserire una registrazione propria in Impostazioni › Integrazioni ›
Avanzate.

Le schermate dei fornitori cambiano nel tempo; i nomi qui sono quelli del
2026.

## Google Calendar

1. Su <https://console.cloud.google.com> crea un progetto (es.
   «Greatnote»).
2. Apri **API e servizi › Libreria**, cerca **Google Calendar API** e
   premi **Abilita**.
3. In **Google Auth Platform › Branding** imposta nome dell'app, email di
   assistenza e contatto dello sviluppatore. Per la pubblicazione servono
   anche:
   - home page e informativa sulla privacy;
   - un dominio autorizzato di tua proprietà.
4. In **Pubblico** scegli **Esterno**. Finché l'app è in **Test**, aggiungi
   gli indirizzi autorizzati in **Utenti di test** (massimo 100).
5. In **Accesso ai dati › Aggiungi o rimuovi ambiti** aggiungi:
   - `https://www.googleapis.com/auth/calendar.events`;
   - `https://www.googleapis.com/auth/calendar.calendarlist.readonly`.
6. In **Client › Crea client** scegli tipo **App desktop**. Copia ID client e
   segreto client. Non serve configurare URI di reindirizzamento: le app
   desktop accettano `http://127.0.0.1` con qualunque porta.

**Stato di pubblicazione:**

- **Test:** funziona solo per gli utenti di test. L'accesso scade dopo
  7 giorni, poi Greatnote chiede di ricollegare l'account.
- **In produzione, non verificata:** chiunque può collegarsi passando dalla
  schermata «app non verificata», con un limite di 100 utenti.
- **Verificata:** nessun avviso e nessun limite. Si chiede da
  **Verifica** in Google Auth Platform. `calendar.events` è un ambito
  sensibile, quindi Google chiede:
  - dominio verificato e informativa sulla privacy;
  - una spiegazione dell'uso degli ambiti;
  - un video dimostrativo del collegamento.

  Non serve la valutazione di sicurezza a pagamento, che riguarda gli ambiti
  «con restrizioni» come Gmail e Drive.

## Outlook / Microsoft 365

1. Su <https://entra.microsoft.com> apri **Applicazioni › Registrazioni
   app › Nuova registrazione**.
2. **Nome:** Greatnote. **Tipi di account supportati:** «Account in
   qualsiasi directory organizzativa e account Microsoft personali».
3. **URI di reindirizzamento:** piattaforma **Client pubblico/nativo (per
   dispositivi mobili e desktop)**, valore `http://localhost`. La porta
   viene ignorata nel confronto.
4. Premi **Registra** e copia l'**ID applicazione (client)**. Non creare
   segreti: Greatnote è un client pubblico con PKCE.
5. Facoltativo: in **Autorizzazioni API** aggiungi le autorizzazioni
   delegate di Microsoft Graph `Calendars.ReadWrite`, `User.Read` e
   `offline_access`. Greatnote le chiede comunque al primo accesso.
6. Facoltativo: la **verifica dell'editore** (con un account Microsoft AI
   Cloud Partner Program) toglie l'etichetta «non verificato» dalla
   schermata di consenso.

Negli account aziendali l'amministratore può richiedere la propria
approvazione prima che gli utenti consentano l'accesso.

## Dropbox (sincronizzazione)

1. Su <https://www.dropbox.com/developers/apps> premi **Create app**.
2. **Scoped access**, poi **App folder**, poi un nome unico (es.
   «Greatnote-Nome»).
3. Nella scheda **Settings**:
   - copia l'**App key** in `GREATNOTE_DROPBOX_APP_KEY` (l'App secret non
     serve);
   - in **OAuth 2 › Redirect URIs** aggiungi `http://localhost:41927/`;
   - imposta **Allow public clients (Implicit Grant & PKCE)** su
     **Allow**.
4. Nella scheda **Permissions** spunta `files.metadata.read`,
   `files.metadata.write`, `files.content.read` e `files.content.write`, poi
   **Submit**.

Finché l'app resta in stato «development», Dropbox ne limita gli utenti; per
la distribuzione pubblica si chiede l'approvazione («Apply for
production»).

## Verifica

1. Compila con le variabili caricate.
2. In Impostazioni › Integrazioni i pulsanti **Collega Google Calendar** e
   **Collega Outlook / Microsoft 365** sono attivi, e la registrazione
   personale resta chiusa tra le opzioni avanzate.
3. Premi **Collega**, accedi e consenti: il browser mostra «Account
   collegato» e il calendario si sincronizza.
