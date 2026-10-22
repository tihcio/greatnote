//! Testo cifrato nelle note (`greatnote_core::secret_text`).
//!
//! La cifratura usa scrypt (circa un secondo di CPU) e gira fuori dal thread
//! dei comandi. Password e testo non vengono mai registrati né salvati: gli
//! errori sono codici (`wrong-passphrase`, …) tradotti dall'interfaccia.

use greatnote_core::secret_text::{self, SecretTextError};
use tauri::AppHandle;

use crate::data_commands::blocking;

type CommandResult<T> = Result<T, String>;

fn code(error: SecretTextError) -> String {
    error.code().to_owned()
}

#[tauri::command]
pub async fn encrypt_note_text(
    app: AppHandle,
    text: String,
    passphrase: String,
) -> CommandResult<String> {
    blocking(app, move |_| {
        secret_text::encrypt(&text, &passphrase).map_err(code)
    })
    .await
}

#[tauri::command]
pub async fn decrypt_note_text(
    app: AppHandle,
    ciphertext: String,
    passphrase: String,
) -> CommandResult<String> {
    blocking(app, move |_| {
        secret_text::decrypt(&ciphertext, &passphrase).map_err(code)
    })
    .await
}
