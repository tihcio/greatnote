# ADR 0028: firma del codice Windows con SignPath Foundation

- **Stato:** accettata, sospesa (richiesta a SignPath Foundation respinta
  il 2026-11-17 per scarsa visibilità del progetto; vedi ADR 0029)
- **Data:** 2026-11-17

## Contesto

Su Windows 11 l'app installata dalle release non si avviava: gli installer
e `Greatnote.exe` non hanno una firma Authenticode, quindi SmartScreen li
segnala e Smart App Control li blocca senza possibilità di proseguire. La
firma minisign degli aggiornamenti (ADR 0023) vale solo per il plugin di
aggiornamento e Windows non la conosce.

## Opzioni valutate

1. **Certificato OV commerciale** (100-300 €/anno): la chiave deve stare su
   token hardware o HSM cloud, scomodo in CI; nessuna reputazione immediata.
2. **Azure Artifact Signing** (circa 10 $/mese): comodo in CI, ma a
   pagamento e con requisiti di idoneità per i singoli sviluppatori.
3. **Microsoft Store (MSIX):** firma di Microsoft, ma gli aggiornamenti
   passerebbero dallo Store e non dall'updater di ADR 0023.
4. **SignPath Foundation:** gratuito per progetti con licenza OSI, firma da
   GitHub Actions con verifica dell'origine della build e approvazione
   manuale. Il certificato è intestato a SignPath Foundation.

## Decisione

Opzione 4. Il job Windows di `installers.yml`, per i tag `v*` e solo se la
variabile `SIGNPATH_ORGANIZATION_ID` è impostata:

1. compila l'app con `tauri build --no-bundle`;
2. invia `Greatnote.exe` a SignPath (configurazione `app`);
3. crea gli installer con `tauri bundle` usando l'eseguibile firmato;
4. invia NSIS e MSI a SignPath (configurazione `installers`);
5. verifica ogni firma con `Get-AuthenticodeSignature`;
6. rigenera le firme minisign (`updater-manifest.ps1 -Resign`), perché la
   firma Authenticode modifica i byte degli installer.

Le configurazioni degli artefatti sono versionate in
`.signpath/artifact-configurations/` e vincolano nome e versione del
prodotto, come richiesto dalle condizioni di SignPath Foundation.

Le condizioni richiedono anche un'informativa privacy mostrata durante
l'installazione per le connessioni non richieste dall'utente (il controllo
degli aggiornamenti): gli installer NSIS e MSI mostrano
`src-tauri/installer/license-and-privacy.txt` tramite `bundle.licenseFile`.
Il README contiene la sezione «Code signing policy» con ruoli e privacy.

## Conseguenze

- Ogni release richiede due approvazioni su SignPath; il workflow attende
  fino a un'ora per ciascuna.
- I binari di llama.cpp non possono essere firmati con questo certificato:
  con Smart App Control l'AI integrata può non avviarsi, mentre l'app sì.
  Valutare in futuro binari llama.cpp firmati dall'upstream.
- `uninstall.exe` di NSIS, generato sul computer dell'utente, non è firmato.
- Senza SignPath configurato il workflow resta quello di prima: le release
  sono possibili anche prima dell'approvazione della richiesta.
- L'editore mostrato da Windows è «SignPath Foundation», non Greatnote.
