//! Identità del pacchetto MSIX (Microsoft Store, ADR 0029).
//!
//! Lo stesso eseguibile gira installato da NSIS/MSI oppure dentro il
//! pacchetto MSIX dello Store. Nel secondo caso gli aggiornamenti li
//! gestisce lo Store e Windows accetta le notifiche solo con l'ID
//! dell'app del pacchetto.
// Su Android e iOS non esistono né pacchetti MSIX né gli aggiornamenti
// dall'app che ne dipendono.
#![cfg_attr(mobile, allow(dead_code))]

use std::sync::OnceLock;

/// Id dell'applicazione nel manifest (`<Application Id="App">`).
#[cfg(windows)]
const APPLICATION_ID: &str = "App";

/// Nome della famiglia del pacchetto, se l'app gira dentro un pacchetto MSIX.
pub fn package_family_name() -> Option<&'static str> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(read_package_family_name).as_deref()
}

/// Vero se l'app è stata installata dal pacchetto MSIX.
pub fn is_packaged() -> bool {
    package_family_name().is_some()
}

/// AppUserModelID dell'app nel pacchetto, richiesto per le notifiche.
#[cfg(windows)]
pub fn app_user_model_id() -> Option<String> {
    package_family_name().map(|family| format!("{family}!{APPLICATION_ID}"))
}

#[cfg(windows)]
fn read_package_family_name() -> Option<String> {
    use windows_sys::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
        Storage::Packaging::Appx::GetCurrentPackageFamilyName,
    };

    let mut length: u32 = 0;
    // SAFETY: con lunghezza 0 e buffer nullo l'API non scrive nel buffer e
    // restituisce in `length` la dimensione necessaria (terminatore incluso),
    // oppure APPMODEL_ERROR_NO_PACKAGE se il processo non ha un pacchetto.
    let result = unsafe { GetCurrentPackageFamilyName(&mut length, std::ptr::null_mut()) };
    if result != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: il buffer ha esattamente `length` elementi, come indicato
    // dall'API nella chiamata precedente, e resta valido durante la chiamata.
    let result = unsafe { GetCurrentPackageFamilyName(&mut length, buffer.as_mut_ptr()) };
    if result != ERROR_SUCCESS {
        return None;
    }
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end])
        .ok()
        .filter(|name| !name.is_empty())
}

#[cfg(not(windows))]
fn read_package_family_name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn tests_do_not_run_inside_a_package() {
        assert!(!super::is_packaged());
        assert_eq!(super::package_family_name(), None);
    }
}
