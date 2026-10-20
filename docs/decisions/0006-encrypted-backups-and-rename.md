# ADR 0006: copie di backup cifrate e nome Greatnote

- **Stato:** accettata
- **Data:** 2026-10-14

## Contesto

La specifica richiede backup con opzione di cifratura e password gestita con
primitive robuste, senza crittografia proprietaria. L'ADR 0005 lasciava aperta
la cifratura perché mancava un modello di chiavi e di recupero. Nello stesso
momento il prodotto cambia nome da "GNA" a "Greatnote".

## Decisione

### Cifratura

- **Formato:** age v1 (`age-encryption.org/v1`) con destinatario scrypt
  (password) tramite il crate `age` 0.12 (MIT/Apache-2.0, implementazione di
  riferimento in Rust). Il file cifrato contiene lo ZIP di backup invariato;
  si può decifrare anche senza l'app (`age -d copia.zip.age > copia.zip`),
  quindi la cifratura non crea lock-in.
- **Ambito:** solo le **copie esportate** ("Copia cifrata…") e il ripristino da
  file esterno. I backup interni in `backups/` restano in chiaro e sono
  protetti dagli stessi permessi (0700/0600) del database attivo: cifrarli
  richiederebbe di salvare la password, oppure di chiederla a ogni avvio per i
  backup automatici.
- **Modello di chiavi:** la password è scelta dall'utente (almeno 10
  caratteri, controllo nel core e nella UI) e non viene mai salvata né
  registrata nei log. **Non esiste recupero**: senza password la copia non è
  leggibile. La UI lo dice prima del salvataggio.
- **Work factor:** quello calibrato dalla libreria (circa 1 s su chi cifra).
  In decifratura vale il limite della libreria (fino a circa 16 volte il target
  locale), che protegge da file ostili costruiti per esaurire CPU e memoria.
- **Ripristino:** il file viene riconosciuto dall'header, decifrato in un file
  privato temporaneo (`.decrypted-<uuid>.zip`, 0600, nella cartella dati) e
  verificato come ogni altro backup. La copia in chiaro viene eliminata dopo il
  ripristino, all'annullamento, con una nuova selezione e, se è rimasta per un
  arresto anomalo, al successivo avvio. Una password errata (rilevata
  dall'header) e un file alterato o troncato (rilevati dall'autenticazione dei
  blocchi) producono errori distinti.

### Nome

- Il nome visibile diventa **Greatnote**, con identificatore Tauri
  `dev.greatnote.notes`; i crate diventano `greatnote-core` e
  `greatnote-desktop`.
- All'avvio, se la vecchia cartella (`dev.gna.notes`) contiene un database e
  quella nuova non ne ha uno, o ne ha uno **senza alcun dato** (nessuna nota,
  taccuino, spazio, tag o bozza), vengono spostati database, allegati e
  backup (`src/data_dir.rs`). La decisione si basa sul database e non
  sull'esistenza della cartella, perché WebKitGTK crea la cartella nuova per
  le proprie cache prima del codice di avvio: una prima versione che
  controllava la cartella saltava lo spostamento. Il database vuoto viene
  conservato in `.replaced-<id>/`, non eliminato. Se entrambe le cartelle
  contengono dati non si tocca nulla. Se uno spostamento fallisce, quelli già
  fatti vengono annullati e l'app usa la vecchia cartella. Le cache del motore
  web restano nella vecchia cartella, che si può eliminare a mano.
- Restano invariati gli **identificatori su disco**: file `gna.sqlite3`,
  formati `gna-archive` e `gna-backup`. Sono formati documentati e già
  presenti in archivi e backup degli utenti; cambiarli avrebbe solo costi di
  compatibilità. I nuovi backup si chiamano `greatnote-<tipo>-….zip`; i nomi
  `gna-<tipo>-….zip` restano riconosciuti per elenco, ripristino e
  conservazione.

## Conseguenze

- Nuova dipendenza del core: `age` (con `scrypt` e `chacha20poly1305` di
  RustCrypto). La libreria si dichiara "beta" come API, mentre il formato age
  v1 è stabile e specificato.
- La password passa dal frontend al processo Rust attraverso l'IPC locale come
  argomento del comando. Non viene salvata, ma la stringa non viene azzerata
  in memoria in modo garantito (né in JavaScript né nel deserializzatore).
- I backup automatici restano non cifrati: per proteggerli serve la
  cifratura del disco di sistema. Una cifratura dei backup interni con chiave
  nel keyring di sistema può essere valutata quando verrà introdotto il
  keyring (fase 5, provider AI).
