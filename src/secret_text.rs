//! Testo cifrato dentro una nota (nodo `encryptedText`).
//!
//! Il testo è cifrato con una password nel formato standard age
//! (`age-encryption.org/v1`, scrypt + ChaCha20-Poly1305) in forma testuale
//! (`-----BEGIN AGE ENCRYPTED FILE-----`), lo stesso delle copie di backup
//! cifrate: resta decifrabile anche con lo strumento `age -d`, senza l'app.
//!
//! La password non viene mai salvata. Il nodo contiene solo il testo cifrato
//! e un suggerimento facoltativo in chiaro; il testo decifrato esiste solo in
//! memoria, nel dialog che lo mostra. Ricerca, AI, sincronizzazione ed
//! esportazioni vedono soltanto il testo cifrato.

use std::io::{Read, Write};

use age::{
    armor::{ArmoredReader, ArmoredWriter, Format},
    secrecy::SecretString,
};

/// Prima e ultima riga del formato testuale di age.
pub const ARMOR_BEGIN: &str = "-----BEGIN AGE ENCRYPTED FILE-----";
pub const ARMOR_END: &str = "-----END AGE ENCRYPTED FILE-----";
/// Lunghezza minima della password.
pub const MIN_PASSPHRASE_CHARS: usize = 8;
/// Testo in chiaro massimo (UTF-8).
pub const MAX_PLAINTEXT_BYTES: usize = 32 * 1024;
/// Testo cifrato massimo: comprende l'header scrypt e la codifica base64.
pub const MAX_CIPHERTEXT_BYTES: usize = 64 * 1024;
/// Suggerimento per ricordare la password, salvato in chiaro.
pub const MAX_HINT_CHARS: usize = 100;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretTextError {
    #[error("La password deve avere almeno {0} caratteri")]
    WeakPassphrase(usize),
    #[error("Il testo da cifrare è vuoto")]
    Empty,
    #[error("Il testo da cifrare supera il limite di 32 KiB")]
    TooLong,
    #[error("Password errata")]
    WrongPassphrase,
    #[error("Il testo cifrato non è valido o è danneggiato")]
    Invalid,
    #[error("La cifratura richiede più risorse di quelle consentite su questo computer")]
    ExcessiveWork,
}

impl SecretTextError {
    /// Codice stabile per l'interfaccia, che mostra il messaggio nella sua
    /// lingua.
    pub fn code(&self) -> &'static str {
        match self {
            Self::WeakPassphrase(_) => "weak-passphrase",
            Self::Empty => "empty",
            Self::TooLong => "too-long",
            Self::WrongPassphrase => "wrong-passphrase",
            Self::Invalid => "invalid",
            Self::ExcessiveWork => "excessive-work",
        }
    }
}

/// Normalizza il testo da cifrare: a capo uniformi e nessun carattere di
/// controllo tranne a capo e tabulazione.
fn normalized(plaintext: &str) -> Result<String, SecretTextError> {
    let text = plaintext.replace("\r\n", "\n").replace('\r', "\n");
    if text.trim().is_empty() {
        return Err(SecretTextError::Empty);
    }
    if text.len() > MAX_PLAINTEXT_BYTES {
        return Err(SecretTextError::TooLong);
    }
    Ok(text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect())
}

/// Cifra `plaintext` con la password e restituisce il testo age armored.
pub fn encrypt(plaintext: &str, passphrase: &str) -> Result<String, SecretTextError> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(SecretTextError::WeakPassphrase(MIN_PASSPHRASE_CHARS));
    }
    let text = normalized(plaintext)?;
    #[cfg_attr(not(test), allow(unused_mut))]
    let mut recipient = age::scrypt::Recipient::new(SecretString::from(passphrase.to_owned()));
    // Nei test un work factor basso evita secondi di CPU per ogni cifratura;
    // in produzione resta quello calibrato da age (circa un secondo).
    #[cfg(test)]
    recipient.set_work_factor(10);
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|_| SecretTextError::Invalid)?;
    let mut output = Vec::new();
    let armored = ArmoredWriter::wrap_output(&mut output, Format::AsciiArmor)
        .map_err(|_| SecretTextError::Invalid)?;
    let mut writer = encryptor
        .wrap_output(armored)
        .map_err(|_| SecretTextError::Invalid)?;
    writer
        .write_all(text.as_bytes())
        .and_then(|()| writer.finish())
        .and_then(ArmoredWriter::finish)
        .map_err(|_| SecretTextError::Invalid)?;
    let armored = String::from_utf8(output).map_err(|_| SecretTextError::Invalid)?;
    let armored = armored.trim_end().to_owned();
    if !is_armored_ciphertext(&armored) {
        return Err(SecretTextError::TooLong);
    }
    Ok(armored)
}

/// Decifra un testo creato da [`encrypt`] (o da `age -p -a`).
pub fn decrypt(armored: &str, passphrase: &str) -> Result<String, SecretTextError> {
    if !is_armored_ciphertext(armored) {
        return Err(SecretTextError::Invalid);
    }
    let decryptor = age::Decryptor::new(ArmoredReader::new(armored.as_bytes()))
        .map_err(|_| SecretTextError::Invalid)?;
    if !decryptor.is_scrypt() {
        return Err(SecretTextError::Invalid);
    }
    let identity = age::scrypt::Identity::new(SecretString::from(passphrase.to_owned()));
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|error| match error {
            age::DecryptError::DecryptionFailed
            | age::DecryptError::KeyDecryptionFailed
            | age::DecryptError::NoMatchingKeys => SecretTextError::WrongPassphrase,
            age::DecryptError::ExcessiveWork { .. } => SecretTextError::ExcessiveWork,
            _ => SecretTextError::Invalid,
        })?;
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_PLAINTEXT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SecretTextError::Invalid)?;
    if bytes.len() > MAX_PLAINTEXT_BYTES {
        return Err(SecretTextError::Invalid);
    }
    String::from_utf8(bytes).map_err(|_| SecretTextError::Invalid)
}

/// Controllo strutturale (senza password) di un testo cifrato salvato in una
/// nota: righe di apertura e chiusura di age, solo caratteri base64 in mezzo
/// e dimensione entro il limite.
pub fn is_armored_ciphertext(value: &str) -> bool {
    let value = value.trim_end();
    if value.len() > MAX_CIPHERTEXT_BYTES {
        return false;
    }
    let Some(body) = value
        .strip_prefix(ARMOR_BEGIN)
        .and_then(|rest| rest.strip_suffix(ARMOR_END))
    else {
        return false;
    };
    let body = body.trim_matches(['\n', '\r']);
    !body.is_empty()
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '\n' | '\r'))
}

/// Suggerimento valido: breve, su una riga.
pub fn is_valid_hint(value: &str) -> bool {
    value.chars().count() <= MAX_HINT_CHARS && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_the_right_password_only() {
        let armored = encrypt("PIN banca: 1234\r\nconto 42", "password lunga").expect("encrypt");
        assert!(armored.starts_with(ARMOR_BEGIN) && armored.ends_with(ARMOR_END));
        assert!(!armored.contains("1234"));
        assert!(is_armored_ciphertext(&armored));
        assert_eq!(
            decrypt(&armored, "password lunga").expect("decrypt"),
            "PIN banca: 1234\nconto 42"
        );
        assert_eq!(
            decrypt(&armored, "password sbagliata"),
            Err(SecretTextError::WrongPassphrase)
        );
        // Ogni cifratura usa un sale diverso.
        assert_ne!(
            armored,
            encrypt("PIN banca: 1234\nconto 42", "password lunga").expect("again")
        );
    }

    #[test]
    fn rejects_weak_passwords_empty_and_oversized_text() {
        assert_eq!(
            encrypt("segreto", "corta"),
            Err(SecretTextError::WeakPassphrase(MIN_PASSPHRASE_CHARS))
        );
        assert_eq!(
            encrypt(" \n ", "password lunga"),
            Err(SecretTextError::Empty)
        );
        let long = "x".repeat(MAX_PLAINTEXT_BYTES + 1);
        assert_eq!(
            encrypt(&long, "password lunga"),
            Err(SecretTextError::TooLong)
        );
        let largest = "é".repeat(MAX_PLAINTEXT_BYTES / 2);
        let armored = encrypt(&largest, "password lunga").expect("largest");
        assert_eq!(
            decrypt(&armored, "password lunga").expect("decrypt"),
            largest
        );
    }

    #[test]
    fn tampered_or_foreign_ciphertext_is_invalid() {
        let armored = encrypt("segreto", "password lunga").expect("encrypt");
        let lines: Vec<&str> = armored.lines().collect();
        let mut body = lines[1].to_owned();
        let flipped = if body.starts_with('A') { "B" } else { "A" };
        body.replace_range(0..1, flipped);
        let tampered = [lines[0], &body]
            .into_iter()
            .chain(lines[2..].iter().copied())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(
            decrypt(&tampered, "password lunga"),
            Err(SecretTextError::Invalid | SecretTextError::WrongPassphrase)
        ));
        assert_eq!(
            decrypt("testo qualsiasi", "password lunga"),
            Err(SecretTextError::Invalid)
        );
        assert!(!is_armored_ciphertext(&format!(
            "{ARMOR_BEGIN}\n<script>\n{ARMOR_END}"
        )));
        assert!(!is_armored_ciphertext(&format!(
            "{ARMOR_BEGIN}\n{ARMOR_END}"
        )));
    }

    #[test]
    fn hints_are_short_single_lines() {
        assert!(is_valid_hint("il nome del gatto"));
        assert!(!is_valid_hint("riga\naltra"));
        assert!(!is_valid_hint(&"x".repeat(MAX_HINT_CHARS + 1)));
    }
}
