//! Lingua dei messaggi prodotti dal nucleo (errori e avvisi mostrati
//! all'utente). L'app la imposta all'avvio dall'ambiente con
//! [`init_from_environment`] e poi l'interfaccia la conferma con
//! [`set_language`]. Se nessuno la imposta vale l'italiano, così i test
//! non dipendono dalla lingua della macchina che li esegue.
//!
//! I messaggi si scrivono con [`tr!`], italiano e inglese affiancati: la
//! scelta avviene quando il messaggio viene formattato, quindi funziona
//! anche dentro `#[error(...)]` di `thiserror`. Vedi ADR 0027.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Italian,
    English,
}

impl Language {
    /// Stessa regola dell'interfaccia: italiano se il codice inizia con
    /// "it", altrimenti inglese.
    pub fn from_code(code: &str) -> Self {
        if code.trim().to_ascii_lowercase().starts_with("it") {
            Self::Italian
        } else {
            Self::English
        }
    }
}

const UNSET: u8 = 0;
const ITALIAN: u8 = 1;
const ENGLISH: u8 = 2;

static CURRENT: AtomicU8 = AtomicU8::new(UNSET);

thread_local! {
    /// Solo per i test: lingua del thread corrente, senza toccare quella
    /// globale condivisa dagli altri test in parallelo.
    static OVERRIDE: Cell<Option<Language>> = const { Cell::new(None) };
}

pub fn set_language(language: Language) {
    let value = match language {
        Language::Italian => ITALIAN,
        Language::English => ENGLISH,
    };
    CURRENT.store(value, Ordering::Relaxed);
}

pub fn language() -> Language {
    if let Some(language) = OVERRIDE.with(Cell::get) {
        return language;
    }
    match CURRENT.load(Ordering::Relaxed) {
        ITALIAN => Language::Italian,
        ENGLISH => Language::English,
        _ => Language::Italian,
    }
}

/// Imposta la lingua da `LC_ALL`, `LC_MESSAGES` o `LANG` (inglese se
/// assenti, come su Windows). L'interfaccia la sostituisce all'avvio.
pub fn init_from_environment() {
    set_language(environment_language());
}

pub fn is_english() -> bool {
    language() == Language::English
}

/// Esegue `f` con la lingua indicata sul thread corrente (per i test).
pub fn with_language<T>(language: Language, f: impl FnOnce() -> T) -> T {
    let previous = OVERRIDE.with(|cell| cell.replace(Some(language)));
    let result = f();
    OVERRIDE.with(|cell| cell.set(previous));
    result
}

fn environment_language() -> Language {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .map_or(Language::English, |value| Language::from_code(&value))
}

/// Messaggio in italiano e inglese; restituisce `String`.
///
/// ```ignore
/// let count = 3;
/// let text = tr!("{} note trovate", "{} notes found", count);
/// ```
///
/// L'esempio non è un doctest: su Windows, compilando l'intero workspace,
/// l'eseguibile separato del doctest non trovava le librerie di sistema al
/// collegamento (`cargo test --workspace` nel workflow Installers). Lo
/// stesso comportamento è verificato da `tr_picks_the_thread_language`.
///
/// Gli argomenti sono posizionali (`{}`), così valgono anche i campi
/// abbreviati di `thiserror`: `#[error("{}", tr!("Codice {}", "Code {}", .0))]`.
#[macro_export]
macro_rules! tr {
    ($it:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::l10n::is_english() {
            format!($en $(, $arg)*)
        } else {
            format!($it $(, $arg)*)
        }
    };
}

/// Come [`tr!`] ma senza formattazione: restituisce uno dei due letterali
/// (`&'static str`), quindi va bene dove serviva il letterale originale
/// (campi `&'static str`, argomenti `&str`).
#[macro_export]
macro_rules! tr_str {
    ($it:literal, $en:literal $(,)?) => {
        if $crate::l10n::is_english() { $en } else { $it }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_follow_the_interface_rule() {
        assert_eq!(Language::from_code("it-IT"), Language::Italian);
        assert_eq!(Language::from_code("IT"), Language::Italian);
        assert_eq!(Language::from_code("it_CH.UTF-8"), Language::Italian);
        assert_eq!(Language::from_code("en-US"), Language::English);
        assert_eq!(Language::from_code("de"), Language::English);
        assert_eq!(Language::from_code(""), Language::English);
    }

    #[test]
    fn tr_picks_the_thread_language() {
        let count = 2;
        let it = with_language(Language::Italian, || tr!("{} note", "{} notes", count));
        let en = with_language(Language::English, || tr!("{} note", "{} notes", count));
        assert_eq!(it, "2 note");
        assert_eq!(en, "2 notes");
    }

    #[test]
    fn tr_str_returns_static_literals() {
        let pick = || -> &'static str { tr_str!("Password errata", "Wrong password") };
        assert_eq!(with_language(Language::Italian, pick), "Password errata");
        assert_eq!(with_language(Language::English, pick), "Wrong password");
    }

    #[test]
    fn override_is_restored() {
        with_language(Language::English, || {
            with_language(Language::Italian, || assert!(!is_english()));
            assert!(is_english());
        });
    }

    #[derive(Debug, thiserror::Error)]
    enum Sample {
        #[error("{}", tr!("Almeno {} caratteri", "At least {} characters", .0))]
        Short(usize),
        #[error("{}", tr!("Codice {}: {}", "Code {}: {}", .status, .message))]
        Http { status: u16, message: String },
    }

    #[test]
    fn works_inside_thiserror() {
        with_language(Language::English, || {
            assert_eq!(Sample::Short(8).to_string(), "At least 8 characters");
        });
        let http = Sample::Http {
            status: 404,
            message: "x".into(),
        };
        with_language(Language::Italian, || {
            assert_eq!(http.to_string(), "Codice 404: x")
        });
    }
}
