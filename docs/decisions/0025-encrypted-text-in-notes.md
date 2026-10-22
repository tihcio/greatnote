# ADR 0025: testo cifrato con password dentro una nota

- **Stato:** accettata
- **Data:** 2026-10-22

## Contesto

L'utente vuole proteggere con una password solo una parte di una nota (un
PIN, una combinazione, dei dati di accesso), come l'opzione di Evernote: la
porzione resta illeggibile e si mostra con un clic e la password scelta al
momento della creazione.

Il testo in chiaro non deve finire nel database, nell'indice di ricerca,
nell'indice semantico, nelle richieste AI, nei lotti di sincronizzazione o
nelle esportazioni.

## Decisione

- **Nodo `encryptedText`** del documento (blocco atomico) con due attributi:
  `ciphertext` (obbligatorio) e `hint` (facoltativo, ≤ 100 caratteri, su una
  riga, salvato in chiaro). Nessun contenuto figlio né contrassegno. La
  validazione Rust controlla la struttura del testo cifrato (righe di
  apertura e chiusura di age, solo base64, ≤ 64 KiB).
- **Formato age con password** (`age-encryption.org/v1`, scrypt +
  ChaCha20-Poly1305) in forma testuale (`-----BEGIN AGE ENCRYPTED FILE-----`),
  con la crate `age` già usata per le copie di backup cifrate (ADR 0006;
  attivata la funzione `armor`, senza nuove dipendenze). Formato aperto e
  documentato: il testo si decifra anche con `age -d`, senza l'app. Nessuna
  crittografia propria.
- **Cifratura e decifratura in Rust** (`greatnote_core::secret_text`, comandi
  `encrypt_note_text` e `decrypt_note_text`) fuori dal thread dei comandi,
  perché scrypt richiede circa un secondo. La password non viene salvata né
  registrata; gli errori sono codici (`wrong-passphrase`, …) tradotti
  dall'interfaccia. Password di almeno 8 caratteri, testo fino a 32 KiB in
  testo semplice (la formattazione della selezione non viene conservata).
- **Interfaccia.** Il comando «Testo cifrato» (menu `/`, «＋ Inserisci»,
  Modifica › Cifra il testo selezionato…, `Ctrl+Shift+X`) apre un dialog
  con il testo (la selezione, se c'è), password, conferma e suggerimento.
  Nella nota resta un riquadro con il lucchetto. Il clic chiede la password
  e mostra il testo nel dialog, che si può copiare, modificare (cifrato di
  nuovo con la stessa password) o riportare in chiaro dopo una conferma.
  Il testo decifrato esiste solo nello stato del dialog.
- **Testo già salvato in chiaro.** Cifrando una selezione, il testo era già
  nelle revisioni della nota. Il dialog propone (attivo per impostazione
  predefinita) di eliminare la cronologia della nota: prima salva la nota
  con il testo cifrato, poi elimina le revisioni con `secure_delete`
  (altrimenti il salvataggio successivo registrerebbe la versione in chiaro
  come nuova revisione).
- **Altri formati.** Testo semplice per ricerca, AI e anteprime: solo «🔒».
  Markdown: blocco ```` ```age ````; HTML: `<pre><code class="language-age">`;
  esportazione in testo semplice: il testo cifrato. Importando Markdown o
  HTML, un blocco `age` con un testo cifrato valido torna un nodo
  `encryptedText` (il suggerimento non viene esportato).
- **Modifiche AI.** L'AI non vede i testi cifrati, quindi «Sostituisci» su un
  intervallo che ne contiene uno inserisce il risultato sotto invece di
  cancellarli.

## Alternative scartate

- **Nodo in linea, dentro un paragrafo** come in Evernote: complica
  esportazioni e importazioni (il testo age è su più righe). Un blocco
  esporta in un formato standard e si reimporta senza perdite.
- **Cifrare il frammento di documento in JSON** per conservare la
  formattazione: con `age -d` si otterrebbe JSON invece del testo, e la
  decifratura richiederebbe di validare un documento non attendibile.
- **Ricordare la password per la sessione:** rimandato; ogni apertura
  chiede la password.

## Conseguenze e limiti

- Una password dimenticata non è recuperabile.
- Il testo già salvato in chiaro prima della cifratura resta nei backup e
  nei lotti di sincronizzazione già scritti (finché non esiste la
  compattazione) e, senza eliminare la cronologia, nelle revisioni.
  Scrivere il testo direttamente nel dialog evita il problema.
- Il suggerimento è in chiaro e visibile a chi apre la nota, ma non entra
  nell'indice di ricerca.
- I testi cifrati di Evernote (`<en-crypt>`, schema diverso) continuano a
  non essere importati.
