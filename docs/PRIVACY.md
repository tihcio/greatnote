# Informativa sulla privacy / Privacy policy

Ultimo aggiornamento / Last updated: 17 novembre 2026 / November 17, 2026

[Italiano](#italiano) · [English](#english)

---

## Italiano

Greatnote è un'applicazione open source (GPL-3.0) per note, attività e
calendario che funziona sul tuo dispositivo. Questa informativa vale per
tutte le versioni: Microsoft Store, installer di GitHub, pacchetti Linux e
Android.

### Dati che restano sul dispositivo

Note, taccuini, tag, allegati, attività, promemoria, eventi, impostazioni,
cronologia dell'assistente AI e backup sono salvati solo sul tuo
dispositivo, nella cartella dati dell'app (su Windows
`%APPDATA%\dev.greatnote.notes`; i modelli AI scaricati in
`%LOCALAPPDATA%\dev.greatnote.notes`). Lo sviluppatore non riceve e non
può leggere questi dati.

Greatnote **non ha account**, **non raccoglie statistiche d'uso né
telemetria** e **non contiene pubblicità**.

### Connessioni senza una tua richiesta

- **Controllo degli aggiornamenti** (solo versioni installate da GitHub):
  l'app scarica periodicamente un piccolo file da github.com per sapere se
  esiste una nuova versione. GitHub riceve l'indirizzo IP e la versione
  installata, nessun dato delle note. Si disattiva in Impostazioni ›
  Aggiornamenti. La versione del Microsoft Store non fa questo controllo:
  gli aggiornamenti li gestisce lo Store.

### Servizi che attivi tu

Queste funzioni sono disattivate finché non le configuri. Quando le usi,
i dati necessari vengono inviati direttamente dal tuo dispositivo al
servizio scelto, secondo l'informativa di quel servizio:

| Funzione | Servizio | Dati inviati |
|---|---|---|
| Sincronizzazione | Dropbox | contenuto della libreria (note, allegati, impostazioni sincronizzate) |
| Calendari collegati | Google Calendar, Microsoft Outlook | eventi creati o modificati nei calendari collegati |
| Abbonamenti a calendari | indirizzo `.ics` indicato da te | solo la richiesta del calendario |
| Assistente AI con provider remoto | il provider configurato da te | il testo della richiesta e le note scelte come contesto, solo dopo il tuo consenso esplicito |
| Download di modelli AI | Hugging Face | solo la richiesta del file del modello |

Le credenziali di questi servizi (token e chiavi API) sono conservate nel
portachiavi del sistema operativo (Gestione credenziali su Windows), mai
in chiaro su file. L'AI integrata e i provider locali (per esempio Ollama)
funzionano sul dispositivo senza inviare dati in rete.

Il server MCP opzionale è accessibile solo ai programmi del tuo
dispositivo che lo avviano, e solo se lo abiliti nelle impostazioni.

### Conservazione e cancellazione

I dati restano finché non li elimini dall'app o non cancelli la cartella
dati. Disinstallare l'app non cancella le note, per evitare perdite
involontarie: per rimuovere tutto elimina anche le cartelle indicate
sopra. Puoi scollegare in ogni momento i servizi esterni dalle
impostazioni, che eliminano anche le relative credenziali.

### Minori

Greatnote non raccoglie dati di nessuno, quindi nemmeno di minori.

### Contatti

Domande o segnalazioni: <https://github.com/tihcio/greatnote/issues>.

### Modifiche

Le modifiche a questa informativa vengono pubblicate in questa pagina, con
la data di aggiornamento. La cronologia completa è nel repository.

---

## English

Greatnote is an open-source (GPL-3.0) notes, tasks and calendar
application that runs on your device. This policy applies to every
edition: Microsoft Store, GitHub installers, Linux packages and Android.

### Data that stays on your device

Notes, notebooks, tags, attachments, tasks, reminders, events, settings,
AI assistant history and backups are stored only on your device, in the
app's data folder (on Windows `%APPDATA%\dev.greatnote.notes`; downloaded
AI models in `%LOCALAPPDATA%\dev.greatnote.notes`). The developer does not
receive and cannot read this data.

Greatnote has **no accounts**, collects **no usage statistics or
telemetry** and contains **no advertising**.

### Connections made without a request from you

- **Update check** (GitHub editions only): the app periodically downloads
  a small file from github.com to learn whether a new version exists.
  GitHub receives your IP address and the installed version, no note data.
  It can be turned off in Settings › Updates. The Microsoft Store edition
  does not perform this check: the Store handles updates.

### Services you enable

These features are off until you configure them. When you use them, the
required data is sent directly from your device to the chosen service,
under that service's privacy policy:

| Feature | Service | Data sent |
|---|---|---|
| Sync | Dropbox | library content (notes, attachments, synced settings) |
| Connected calendars | Google Calendar, Microsoft Outlook | events created or changed in the connected calendars |
| Calendar subscriptions | the `.ics` address you enter | only the calendar request |
| AI assistant with a remote provider | the provider you configure | the request text and the notes chosen as context, only after your explicit consent |
| AI model downloads | Hugging Face | only the request for the model file |

Credentials for these services (tokens and API keys) are kept in the
operating system's keyring (Credential Manager on Windows), never in plain
text files. The built-in AI and local providers (for example Ollama) run
on the device without sending data over the network.

The optional MCP server is reachable only by programs on your device that
start it, and only if you enable it in the settings.

### Retention and deletion

Data stays until you delete it in the app or remove the data folder.
Uninstalling the app does not delete your notes, to avoid accidental loss:
to remove everything, also delete the folders listed above. You can
disconnect external services at any time in the settings, which also
deletes their credentials.

### Children

Greatnote collects no data from anyone, including children.

### Contact

Questions or reports: <https://github.com/tihcio/greatnote/issues>.

### Changes

Changes to this policy are published on this page with the update date.
The full history is in the repository.
