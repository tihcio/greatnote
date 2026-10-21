//! Funzioni AI facoltative (Fase 5, ADR 0010).
//!
//! Tutto è disattivato per impostazione predefinita. Il nucleo non dipende da
//! un servizio specifico: [`provider::AiProvider`] descrive chat ed
//! embedding, [`provider::OpenAiCompatible`] lo implementa per i server con
//! API compatibile OpenAI (Ollama, LM Studio, llama.cpp, servizi remoti).
//! [`builtin`] avvia un motore llama.cpp distribuito con l'app, con modelli
//! scaricati su richiesta (nessun server da installare).
//! Il contenuto delle note inviato ai modelli è sempre riletto dal database e
//! trattato come dato non attendibile ([`prompt`]); nessuna funzione AI
//! modifica le note: le proposte vengono applicate solo dall'utente.

pub mod builtin;
pub mod chunking;
pub mod claude_code;
pub mod history;
pub mod index;
pub mod planner_context;
pub mod prompt;
pub mod provider;
pub mod retrieval;
pub mod settings;

use thiserror::Error;

use crate::storage::StorageError;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("Le funzioni AI sono disattivate: attivale in Impostazioni › Intelligenza artificiale")]
    Disabled,
    #[error("Configurazione AI incompleta: {0}")]
    NotConfigured(&'static str),
    #[error("Impostazioni AI non valide: {0}")]
    InvalidSettings(&'static str),
    #[error(
        "Il server AI {0} non è sul tuo computer: per inviargli dati devi dare il consenso nelle impostazioni"
    )]
    RemoteConsentRequired(String),
    #[error("Impossibile contattare il server AI: {0}")]
    Connection(String),
    #[error("Il server AI non ha risposto entro il tempo massimo")]
    Timeout,
    #[error("Il server AI ha rifiutato l'accesso (codice {0}): controlla la chiave API")]
    Unauthorized(u16),
    #[error("Troppe richieste al server AI: riprova {}", retry_hint(*.0))]
    RateLimited(Option<u64>),
    #[error("Il server AI ha risposto con un errore (codice {status}): {message}")]
    Http { status: u16, message: String },
    #[error("Risposta del server AI non valida: {0}")]
    InvalidResponse(String),
    #[error("Richiesta annullata")]
    Cancelled,
    #[error("Testo troppo lungo per l'AI: massimo {0} caratteri")]
    InputTooLarge(usize),
    #[error("Questa nota è esclusa dalle funzioni AI (taccuino escluso nelle impostazioni)")]
    ExcludedNote,
    #[error("Portachiavi di sistema non disponibile: {0}")]
    Keyring(String),
    #[error("Claude Code: {0}")]
    ClaudeCode(String),
    #[error("Lettura dell'immagine non riuscita: {0}")]
    Ocr(String),
    #[error("Motore AI integrato: {0}")]
    Builtin(String),
    #[error("Download del modello non riuscito: {0}")]
    ModelDownload(String),
    #[error("Hugging Face: {0}")]
    Hub(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

fn retry_hint(seconds: Option<u64>) -> String {
    match seconds {
        Some(seconds) => format!("tra {seconds} secondi"),
        None => "più tardi".to_owned(),
    }
}
