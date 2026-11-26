//! Cifratura della libreria di sincronizzazione con una passphrase.
//!
//! - Una **chiave della libreria** casuale di 256 bit cifra tutti i file.
//!   Nella cartella c'è solo cifrata con la passphrase, nel formato age
//!   (scrypt + ChaCha20-Poly1305, lo stesso dei backup e del testo cifrato):
//!   cambiare passphrase riscrive soltanto quel file.
//! - Ogni file è cifrato con XChaCha20-Poly1305, nonce casuale di 192 bit.
//!   I dati associati sono l'id della libreria e il percorso del file: un
//!   file spostato, copiato da un'altra libreria o modificato non si decifra.
//! - Il nome dei blob degli allegati è un HMAC del loro SHA-256, così la
//!   cartella non rivela quali file noti contiene.
//! - Le sottochiavi (cifratura, nomi, identificativo) derivano dalla chiave
//!   con HMAC-SHA256 ed etichette distinte.
//!
//! Restano visibili a chi ospita la cartella: numero, dimensione e data dei
//! file, gli identificativi casuali dei dispositivi e quante modifiche
//! fanno (ADR 0032).

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use crate::secret_text::{self, SecretTextError};

/// Schema di cifratura registrato in `format.json`.
pub const SCHEME: &str = "xchacha20poly1305-hmac-sha256-v1";
/// Passphrase minima: la cartella può stare presso un servizio esterno,
/// dove chi la copia può provare password senza limiti.
pub const MIN_PASSPHRASE_CHARS: usize = 12;

const MAGIC: &[u8; 4] = b"GNS1";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const HEADER_LEN: usize = MAGIC.len() + NONCE_LEN + 8;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("{}", crate::tr!("La passphrase deve avere almeno {} caratteri", "The passphrase must be at least {} characters", .0))]
    WeakPassphrase(usize),
    #[error("{}", crate::tr!("Passphrase errata", "Wrong passphrase"))]
    WrongPassphrase,
    #[error("{}", crate::tr!("file cifrato non valido o modificato", "encrypted file invalid or tampered with"))]
    Invalid,
    #[error("{}", crate::tr!("La cifratura richiede più risorse di quelle consentite su questo dispositivo", "The encryption requires more resources than allowed on this device"))]
    ExcessiveWork,
}

/// Chiave della libreria. Azzerata in memoria quando non serve più.
#[derive(Clone, PartialEq, Eq)]
pub struct LibraryKey([u8; 32]);

impl Drop for LibraryKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for LibraryKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "LibraryKey({})", self.key_id())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

pub fn check_passphrase(passphrase: &str) -> Result<(), CryptoError> {
    if passphrase.trim().chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(CryptoError::WeakPassphrase(MIN_PASSPHRASE_CHARS));
    }
    Ok(())
}

impl LibraryKey {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    fn subkey(&self, label: &str) -> Zeroizing<[u8; 32]> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.0)
            .unwrap_or_else(|_| unreachable!("HMAC accetta chiavi di ogni lunghezza"));
        mac.update(label.as_bytes());
        Zeroizing::new(mac.finalize().into_bytes().into())
    }

    /// Identificativo pubblico della chiave: riconosce una chiave salvata
    /// nel portachiavi senza chiedere la passphrase.
    pub fn key_id(&self) -> String {
        hex(&self.subkey("greatnote-sync key-id v1")[..16])
    }

    /// Nome del blob di un allegato.
    pub fn blob_name(&self, sha256: &str) -> String {
        let key = self.subkey("greatnote-sync blob-names v1");
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key.as_slice())
            .unwrap_or_else(|_| unreachable!("HMAC accetta chiavi di ogni lunghezza"));
        mac.update(sha256.as_bytes());
        hex(&mac.finalize().into_bytes())
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        let key = self.subkey("greatnote-sync file-encryption v1");
        XChaCha20Poly1305::new(key.as_slice().into())
    }

    /// Cifra il contenuto di un file della libreria.
    pub fn seal(&self, library: &str, path: &str, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let aad = associated_data(library, path);
        // L'unico errore possibile è un testo oltre i limiti di
        // XChaCha20 (256 GiB), impossibile per i file della libreria.
        let ciphertext = self
            .cipher()
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .unwrap_or_default();
        let mut output = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&(plaintext.len() as u64).to_le_bytes());
        output.extend_from_slice(&ciphertext);
        output
    }

    /// Decifra un file. `Ok(None)` se il file è più corto di quanto dichiara
    /// (ancora in trasferimento); errore se è stato alterato, appartiene a
    /// un altro percorso o a un'altra chiave.
    pub fn open(
        &self,
        library: &str,
        path: &str,
        bytes: &[u8],
    ) -> Result<Option<Vec<u8>>, CryptoError> {
        if bytes.len() < HEADER_LEN {
            return if MAGIC.starts_with(&bytes[..bytes.len().min(MAGIC.len())]) {
                Ok(None)
            } else {
                Err(CryptoError::Invalid)
            };
        }
        if &bytes[..MAGIC.len()] != MAGIC {
            return Err(CryptoError::Invalid);
        }
        let nonce = &bytes[MAGIC.len()..MAGIC.len() + NONCE_LEN];
        let mut length = [0u8; 8];
        length.copy_from_slice(&bytes[MAGIC.len() + NONCE_LEN..HEADER_LEN]);
        let expected = usize::try_from(u64::from_le_bytes(length))
            .ok()
            .and_then(|length| length.checked_add(HEADER_LEN + TAG_LEN))
            .ok_or(CryptoError::Invalid)?;
        if bytes.len() < expected {
            return Ok(None);
        }
        if bytes.len() > expected {
            return Err(CryptoError::Invalid);
        }
        let aad = associated_data(library, path);
        self.cipher()
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: &bytes[HEADER_LEN..],
                    aad: &aad,
                },
            )
            .map(Some)
            .map_err(|_| CryptoError::Invalid)
    }

    /// Chiave cifrata con la passphrase (formato age testuale).
    pub fn wrap(&self, passphrase: &str) -> Result<String, CryptoError> {
        check_passphrase(passphrase)?;
        let text = Zeroizing::new(hex(&self.0));
        secret_text::encrypt(&text, passphrase).map_err(|error| match error {
            SecretTextError::ExcessiveWork => CryptoError::ExcessiveWork,
            _ => CryptoError::Invalid,
        })
    }

    pub fn unwrap(wrapped: &str, passphrase: &str) -> Result<Self, CryptoError> {
        let text =
            Zeroizing::new(secret_text::decrypt(wrapped, passphrase).map_err(
                |error| match error {
                    SecretTextError::WrongPassphrase => CryptoError::WrongPassphrase,
                    SecretTextError::ExcessiveWork => CryptoError::ExcessiveWork,
                    _ => CryptoError::Invalid,
                },
            )?);
        Self::from_hex(&text).ok_or(CryptoError::Invalid)
    }

    /// Forma testuale per il portachiavi del sistema.
    pub fn to_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(hex(&self.0))
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        let bytes = Zeroizing::new(unhex(text.trim())?);
        let array: [u8; 32] = bytes.as_slice().try_into().ok()?;
        Some(Self(array))
    }
}

fn associated_data(library: &str, path: &str) -> Vec<u8> {
    format!("greatnote-sync v1\0{library}\0{path}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &str = "6f1c1c8e-6d3c-4c3e-9a55-1f6a1f6a1f6a";

    #[test]
    fn files_round_trip_and_are_bound_to_path_and_library() {
        let key = LibraryKey::generate();
        let sealed = key.seal(
            LIBRARY,
            "devices/a/changes/000000000001.json",
            b"{\"ops\":[]}",
        );
        assert!(!sealed.windows(5).any(|window| window == b"\"ops\""));
        assert_eq!(
            key.open(LIBRARY, "devices/a/changes/000000000001.json", &sealed)
                .expect("open"),
            Some(b"{\"ops\":[]}".to_vec())
        );
        // Altro percorso, altra libreria, altra chiave, byte alterati.
        assert_eq!(
            key.open(LIBRARY, "devices/a/changes/000000000002.json", &sealed),
            Err(CryptoError::Invalid)
        );
        assert_eq!(
            key.open("altra", "devices/a/changes/000000000001.json", &sealed),
            Err(CryptoError::Invalid)
        );
        assert_eq!(
            LibraryKey::generate().open(LIBRARY, "devices/a/changes/000000000001.json", &sealed),
            Err(CryptoError::Invalid)
        );
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert_eq!(
            key.open(LIBRARY, "devices/a/changes/000000000001.json", &tampered),
            Err(CryptoError::Invalid)
        );
        // Nonce diverso a ogni cifratura.
        assert_ne!(
            sealed,
            key.seal(
                LIBRARY,
                "devices/a/changes/000000000001.json",
                b"{\"ops\":[]}"
            )
        );
    }

    #[test]
    fn truncated_files_wait_and_foreign_files_are_rejected() {
        let key = LibraryKey::generate();
        let sealed = key.seal(LIBRARY, "x", &[7u8; 1000]);
        for cut in [0, 2, HEADER_LEN, sealed.len() - 1] {
            assert_eq!(key.open(LIBRARY, "x", &sealed[..cut]), Ok(None), "{cut}");
        }
        let mut longer = sealed.clone();
        longer.push(0);
        assert_eq!(key.open(LIBRARY, "x", &longer), Err(CryptoError::Invalid));
        assert_eq!(
            key.open(LIBRARY, "x", b"{\"plain\": true}"),
            Err(CryptoError::Invalid)
        );
    }

    #[test]
    fn key_is_wrapped_with_the_passphrase() {
        let key = LibraryKey::generate();
        assert_eq!(
            key.wrap("corta"),
            Err(CryptoError::WeakPassphrase(MIN_PASSPHRASE_CHARS))
        );
        let wrapped = key.wrap("una passphrase lunga").expect("wrap");
        assert!(!wrapped.contains(key.to_hex().as_str()));
        assert_eq!(
            LibraryKey::unwrap(&wrapped, "una passphrase lunga").expect("unwrap"),
            key
        );
        assert_eq!(
            LibraryKey::unwrap(&wrapped, "un'altra passphrase"),
            Err(CryptoError::WrongPassphrase)
        );
        assert_eq!(LibraryKey::from_hex(&key.to_hex()), Some(key.clone()));
        assert_eq!(LibraryKey::from_hex("zz"), None);
        assert_eq!(key.key_id().len(), 32);
        assert_ne!(key.key_id(), LibraryKey::generate().key_id());
        let name = key.blob_name(&"a".repeat(64));
        assert_eq!(name.len(), 64);
        assert_ne!(name, "a".repeat(64));
        assert_eq!(name, key.blob_name(&"a".repeat(64)));
        assert!(!format!("{key:?}").contains(key.to_hex().as_str()));
    }
}
