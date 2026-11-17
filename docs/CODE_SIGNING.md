# Firma del codice su Windows (SignPath Foundation)

> **Stato (17 novembre 2026): non attiva.** SignPath Foundation ha
> respinto la richiesta perché il progetto non ha ancora abbastanza
> visibilità pubblica (stelle, fork, contributori, citazioni esterne). Si
> può ripresentare più avanti, oppure usare un abbonamento SignPath a
> pagamento: il workflow resta pronto e si attiva da solo con le variabili
> descritte sotto. Prima di una nuova richiesta va rimessa nel README la
> sezione «Code signing policy» (vedi la cronologia di Git) e la menzione
> della firma nella pagina di licenza dell'installer. Su Windows la via
> attuale è il pacchetto MSIX del Microsoft Store
> ([MICROSOFT_STORE.md](MICROSOFT_STORE.md)).

Windows blocca o segnala gli eseguibili scaricati che non hanno una firma
Authenticode: SmartScreen mostra «Windows ha protetto il PC» e Smart App
Control impedisce del tutto l'avvio. La firma minisign degli aggiornamenti
(ADR 0023) protegge solo gli aggiornamenti in-app e Windows non la conosce.

Greatnote usa il programma gratuito per progetti open source di
[SignPath Foundation](https://signpath.org). La decisione è descritta
nell'[ADR 0028](decisions/0028-windows-code-signing-signpath.md).

## Cosa viene firmato

| File | Firmato | Note |
|---|---|---|
| `Greatnote.exe` (l'app installata) | sì | prima richiesta di firma, configurazione `app` |
| `Greatnote_<versione>_x64-setup.exe` (NSIS) | sì | seconda richiesta, configurazione `installers` |
| `Greatnote_<versione>_x64_<lingua>.msi` | sì | seconda richiesta, configurazione `installers` |
| `uninstall.exe` creato da NSIS durante l'installazione | no | generato sul computer dell'utente, non passa da SignPath |
| `llama-server.exe` e DLL di llama.cpp | no | binari upstream: le condizioni di SignPath Foundation vietano di firmarli con il nostro certificato |
| Pacchetti Linux, APK Android | no | fuori dall'ambito di questa firma |

L'editore mostrato da Windows è **SignPath Foundation**, che emette il
certificato per conto dei progetti open source.

Con Smart App Control attivo, l'AI integrata può non avviarsi perché
`llama-server.exe` non è firmato: l'app si apre comunque e mostra l'errore
del motore AI. Restano disponibili i provider AI esterni (ad esempio
Ollama) configurati dall'utente.

## Flusso nel workflow

Solo per i tag `v*` e solo se la variabile del repository
`SIGNPATH_ORGANIZATION_ID` è impostata (altrimenti la release esce non
firmata, come prima):

1. `npx tauri build --no-bundle` compila `target/release/Greatnote.exe`.
2. L'eseguibile viene caricato come artefatto e inviato a SignPath
   (configurazione `app`). Il workflow attende l'approvazione fino a un'ora,
   poi verifica la firma (`Get-AuthenticodeSignature`).
3. `npx tauri bundle` crea NSIS e MSI con l'eseguibile firmato.
4. Gli installer vengono inviati a SignPath (configurazione `installers`),
   verificati e rimessi al loro posto.
5. `scripts/updater-manifest.ps1 -Resign` rigenera le firme minisign
   sugli installer definitivi: la firma Authenticode cambia i byte del file,
   quindi le firme create dal bundle non sarebbero più valide.
6. Viene creata la release in bozza, come prima.

Ogni release richiede quindi **due approvazioni** su SignPath.

## Configurazione (una volta)

1. **Requisiti del progetto.** Licenza OSI (GPL-3.0-only), repository
   pubblico, release già pubblicate, funzionalità descritte nella pagina di
   download, sezione «Code signing policy» nel README (da ripristinare). Tutti i
   membri del team devono usare l'autenticazione a due fattori su GitHub e
   su SignPath.
2. **Richiesta a SignPath Foundation** dal modulo su
   <https://signpath.org/apply>. Dopo l'approvazione si riceve
   un'organizzazione su SignPath.io con un progetto e le policy
   `test-signing` e `release-signing`.
3. **Su SignPath.io:**
   - aggiungere il Trusted Build System predefinito **GitHub.com**
     all'organizzazione e collegarlo al progetto;
   - creare due configurazioni dell'artefatto con slug `app` e
     `installers`, incollando il contenuto di
     [.signpath/artifact-configurations/app.xml](../.signpath/artifact-configurations/app.xml)
     e
     [.signpath/artifact-configurations/installers.xml](../.signpath/artifact-configurations/installers.xml);
   - creare un utente CI e il suo token API, con il ruolo di
     *submitter* sulle policy di firma.
4. **Su GitHub › Settings › Secrets and variables › Actions:**

   | Tipo | Nome | Valore |
   |---|---|---|
   | segreto | `SIGNPATH_API_TOKEN` | token dell'utente CI di SignPath |
   | variabile | `SIGNPATH_ORGANIZATION_ID` | ID dell'organizzazione SignPath |
   | variabile | `SIGNPATH_PROJECT_SLUG` | slug del progetto (es. `greatnote`) |
   | variabile | `SIGNPATH_SIGNING_POLICY_SLUG` | `test-signing` per la prima prova, poi `release-signing` |

   Il token è l'unico segreto: non va mai scritto nel repository o nei log.
5. **Prima prova** con `test-signing`: creare un tag di prova, approvare le
   due richieste e controllare che la firma sia presente (il certificato di
   test non è attendibile per Windows, è normale). Se SignPath rifiuta un
   file per i metadati (`product-name` o `product-version`), confrontare i
   valori con *Proprietà › Dettagli* del file e correggere la
   configurazione. Poi passare a `release-signing`.

## Verifica di un file firmato

``` powershell
Get-AuthenticodeSignature .\Greatnote_0.2.8_x64-setup.exe | Format-List
```

`Status` deve essere `Valid` e il certificato deve indicare SignPath
Foundation. Da *Proprietà › Firme digitali* si vede anche il timestamp.

## Reputazione SmartScreen

La firma toglie subito il blocco di Smart App Control. SmartScreen può
comunque mostrare l'avviso per le prime release, finché il certificato non
accumula reputazione con i download: il certificato di SignPath
Foundation è condiviso da molti progetti, quindi in genere la reputazione
arriva in fretta.
