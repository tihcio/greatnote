//! Segreti dell'app (chiave API dei provider AI, token di Dropbox e dei
//! calendari) nel portachiavi del sistema operativo, mai su file.
//!
//! - Linux: Secret Service; Windows: Credential Manager; macOS: Keychain
//!   (crate `keyring`).
//! - Android: preferenze cifrate con una chiave dell'Android Keystore
//!   (`android-native-keyring-store`). L'API `v1` di `keyring` lì non ha uno
//!   store, quindi si passa da `keyring-core`.
//!
//! Se il portachiavi non è disponibile le funzioni restituiscono un errore:
//! non esiste un ripiego su file in chiaro.

type SecretResult<T> = Result<T, String>;

#[cfg(not(target_os = "android"))]
mod platform {
    pub use keyring::{Entry, Error};

    pub fn entry(service: &str, user: &str) -> Result<Entry, Error> {
        Entry::new(service, user)
    }
}

#[cfg(target_os = "android")]
mod platform {
    use std::sync::OnceLock;

    pub use keyring_core::{Entry, Error};

    /// Lo store si crea una volta: usa il contesto Android che Tauri ha già
    /// inizializzato per il processo.
    fn store_ready() -> Result<(), Error> {
        static READY: OnceLock<Result<(), String>> = OnceLock::new();
        READY
            .get_or_init(|| {
                android_native_keyring_store::Store::new()
                    .map(|store| keyring_core::set_default_store(store))
                    .map_err(|error| error.to_string())
            })
            .clone()
            .map_err(|message| Error::NoStorageAccess(message.into()))
    }

    pub fn entry(service: &str, user: &str) -> Result<Entry, Error> {
        store_ready()?;
        Entry::new(service, user)
    }
}

use platform::{Error, entry};

fn describe(error: Error) -> String {
    error.to_string()
}

/// Legge un segreto; `None` se non è mai stato salvato.
pub fn read(service: &str, user: &str) -> SecretResult<Option<String>> {
    match entry(service, user).map_err(describe)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(describe(error)),
    }
}

/// Salva o sostituisce un segreto.
pub fn write(service: &str, user: &str, secret: &str) -> SecretResult<()> {
    entry(service, user)
        .map_err(describe)?
        .set_password(secret)
        .map_err(describe)
}

/// Elimina un segreto; non è un errore se non esiste.
pub fn delete(service: &str, user: &str) -> SecretResult<()> {
    match entry(service, user).map_err(describe)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(describe(error)),
    }
}
