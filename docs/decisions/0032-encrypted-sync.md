# ADR 0032: sincronizzazione cifrata con passphrase

- **Stato:** accettata
- **Data:** 2026-11-26

## Contesto

La libreria di sincronizzazione (ADR 0017) è leggibile da chi ospita la
cartella: il servizio di sincronizzazione, Dropbox (ADR 0018), o chiunque
ottenga una copia dei file. La specifica chiede segreti nel portachiavi,
nessuna crittografia proprietaria e di non chiamare «end-to-end» una
cifratura che il server può decifrare.

Qui non c'è un server: c'è solo una cartella di file. Una cifratura con una
passphrase nota soltanto ai dispositivi dell'utente è quindi davvero
end-to-end rispetto al servizio che ospita la cartella. La chiamiamo
comunque «cifratura con passphrase», perché è la descrizione più precisa.

## Decisione

### Chiavi

- La **chiave della libreria** è casuale, di 256 bit, e cifra tutti i file.
- Nella cartella la chiave è presente solo **cifrata con la passphrase**, nel
  formato standard age (scrypt con il work factor calibrato da age, più
  ChaCha20-Poly1305). È lo stesso formato dei backup cifrati e del testo
  cifrato nelle note (ADR 0006, 0025). Cambiare passphrase riscrive soltanto
  quel file: gli altri dispositivi continuano con la chiave che hanno.
- **Sottochiavi** distinte (cifratura dei file, nomi dei blob, identificativo
  della chiave), derivate con HMAC-SHA256 ed etichette diverse.
- **Passphrase di almeno 12 caratteri.** Chi copia la cartella può provare
  password senza limiti; il costo di scrypt rende lento ogni tentativo.
- **Portachiavi.** Su ogni dispositivo la chiave della libreria (mai la
  passphrase) sta nel portachiavi del sistema
  (`greatnote-sync/library-key`); su Android sta nelle preferenze cifrate
  con l'Android Keystore. Con la sincronizzazione disattivata o Dropbox
  scollegato viene cancellata.
- **Identificativo pubblico** (`key_id`). Permette di riconoscere una chiave
  salvata senza chiedere la passphrase e di accorgersi se la libreria è
  stata ricreata con un'altra chiave.

### File

- Ogni file (lotti, `device.json`, `retired.json`, blob degli allegati) è
  cifrato con **XChaCha20-Poly1305**, con un nonce casuale di 192 bit.
- Struttura: `GNS1` | nonce | lunghezza del testo in chiaro (u64) | testo
  cifrato con tag.
- **Dati associati:** versione dello schema, id della libreria e percorso
  del file. Un file spostato, copiato da un'altra libreria o modificato non
  si decifra.
- **File troncati.** La lunghezza dichiarata distingue un file ancora in
  trasferimento (più corto: si riprova al giro dopo) da uno alterato
  (errore, lotto saltato e segnalato).
- **Nomi dei blob.** Sono un HMAC dello SHA-256 dell'allegato: la cartella
  non rivela se contiene un file noto. Il contenuto decifrato viene comunque
  verificato con lo SHA-256 prima di salvarlo.
- **Nessuna dipendenza nuova:** `chacha20poly1305`, `hmac` e `zeroize`
  erano già nel lockfile tramite `age`. Le chiavi in memoria vengono
  azzerate quando non servono più.

### Struttura nella cartella

```text
greatnote-sync/
├── format.json            {"format": 2, "library": "<id cifrata>", "previous_library": "<id in chiaro>"?}
├── encrypted/
│   ├── format.json        {"format": 2, "library": "<id>", "encryption": {scheme, key_id, wrapped_key}}
│   ├── devices/<id>/…     come nella libreria in chiaro, ma cifrati
│   └── blobs/<aa>/<hmac>
└── devices/, blobs/       eventuale libreria in chiaro di prima (non più usata)
```

- Il `format.json` principale passa al **formato 2**. Le versioni precedenti
  rifiutano un formato più recente: si fermano con un avviso invece di
  scrivere in chiaro accanto alla libreria cifrata.
- I lotti mantengono il formato 1: cambia solo come vengono conservati.

### Creazione, conversione, accesso

- **Libreria nuova.** Con «Cifra la sincronizzazione» scelto, la prima
  sincronizzazione chiede di scegliere la passphrase (due volte) e crea
  `encrypted/` prima di scrivere il formato principale.
- **Conversione di una libreria in chiaro.** Il dispositivo crea la
  libreria cifrata e riscrive il formato principale con `previous_library`.
  È l'unica volta in cui `format.json` viene riscritto. Poi azzera la
  propria storia del protocollo e riesporta tutto. Gli altri dispositivi
  trovano il formato 2, chiedono la passphrase e fanno lo stesso: le note
  vengono unite come quando un dispositivo entra in una libreria esistente.
  Non si perdono nemmeno le modifiche mai arrivate nella libreria in chiaro.
- **La libreria in chiaro non viene cancellata.** Greatnote non la usa più;
  l'utente può eliminarla a mano quando tutti i dispositivi sono passati
  (la guida lo spiega). Cancellarla in automatico significherebbe eliminare
  file che altri dispositivi stanno forse ancora leggendo.
- **Accesso da un nuovo dispositivo.** Senza una chiave valida la
  sincronizzazione si ferma con «serve la passphrase» (stato
  `passphrase_required`); l'interfaccia mostra il campo. Una passphrase
  errata viene segnalata senza scrivere nulla nella cartella.
- **Corse tra dispositivi.** I file di formato si scrivono con «crea solo se
  manca». Se due dispositivi creano una libreria nello stesso momento,
  prevale il primo e il secondo la apre. In caso di passphrase diverse il
  secondo riceve «passphrase errata».

## Alternative scartate

- **Chiave derivata direttamente dalla passphrase:** cambiarla
  richiederebbe di ricifrare tutta la libreria su tutti i dispositivi.
- **age per ogni file**, con la passphrase (scrypt) o con destinatari
  X25519:
  - con scrypt costerebbe circa un secondo per file;
  - con X25519 chiunque conosca la chiave pubblica potrebbe creare file
    validi, e non c'è modo di legarli al loro percorso.
- **Cifrare la libreria esistente sul posto:** violerebbe l'immutabilità
  dei lotti e la regola di un solo scrittore per cartella, e i dispositivi
  non aggiornati scriverebbero nuovi lotti in chiaro.
- **Tornare in chiaro:** non previsto. Per smettere di cifrare si sceglie
  un'altra cartella (o si disattiva e riattiva su Dropbox, dopo aver
  eliminato la libreria).

## Conseguenze e limiti

- **Metadati visibili** a chi ospita la cartella:
  - numero, dimensione e date dei file;
  - gli identificativi casuali dei dispositivi;
  - quanti lotti scrive ciascuno.

  Non sono visibili titoli, testo, nomi dei dispositivi, nomi e hash degli
  allegati.
- **Passphrase dimenticata:** i dati nella cartella non si recuperano. Le
  note restano su ogni dispositivo, e un dispositivo che ha ancora la chiave
  nel portachiavi può impostare una passphrase nuova.
- **Chi ha accesso in scrittura alla cartella senza la chiave** può
  cancellare o danneggiare file, ma non leggerli né crearne di validi: i
  file alterati vengono rifiutati e segnalati.
- **Cambiare passphrase non revoca un dispositivo** che conosce già la
  chiave. Per escluderne uno serve una nuova libreria (cartella nuova).
