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
pub mod whisper;

use thiserror::Error;

use crate::storage::StorageError;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("{}", crate::tr!("Le funzioni AI sono disattivate: attivale in Impostazioni › Intelligenza artificiale", "AI features are turned off: turn them on in Settings › Artificial intelligence"))]
    Disabled,
    #[error("{}", crate::tr!("Configurazione AI incompleta: {0}", "Incomplete AI configuration: {0}", .0))]
    NotConfigured(&'static str),
    #[error("{}", crate::tr!("Impostazioni AI non valide: {0}", "Invalid AI settings: {0}", .0))]
    InvalidSettings(&'static str),
    #[error(
        "{}",
        crate::tr!(
            "Il server AI {} non è sul tuo computer: per inviargli dati devi dare il consenso nelle impostazioni",
            "The AI server {} is not on your computer: to send it data you must give your consent in the settings",
            .0
        )
    )]
    RemoteConsentRequired(String),
    #[error("{}", crate::tr!("Impossibile contattare il server AI: {0}", "Unable to contact the AI server: {0}", .0))]
    Connection(String),
    #[error("{}", crate::tr!("Il server AI non ha risposto entro il tempo massimo", "The AI server did not respond in time"))]
    Timeout,
    #[error("{}", crate::tr!("Il server AI ha rifiutato l'accesso (codice {0}): controlla la chiave API", "The AI server rejected access (code {0}): check the API key", .0))]
    Unauthorized(u16),
    #[error("{}", crate::tr!("Troppe richieste al server AI: riprova {}", "Too many requests to the AI server: try again {}", retry_hint(*.0)))]
    RateLimited(Option<u64>),
    #[error("{}", crate::tr!("Il server AI ha risposto con un errore (codice {0}): {1}", "The AI server responded with an error (code {0}): {1}", .status, .message))]
    Http { status: u16, message: String },
    #[error("{}", crate::tr!("Risposta del server AI non valida: {0}", "Invalid response from the AI server: {0}", .0))]
    InvalidResponse(String),
    #[error("{}", crate::tr!("Richiesta annullata", "Request cancelled"))]
    Cancelled,
    #[error("{}", crate::tr!("Testo troppo lungo per l'AI: massimo {0} caratteri", "Text too long for AI: maximum {0} characters", .0))]
    InputTooLarge(usize),
    #[error("{}", crate::tr!("Questa nota è esclusa dalle funzioni AI (taccuino escluso nelle impostazioni)", "This note is excluded from AI features (its notebook is excluded in the settings)"))]
    ExcludedNote,
    #[error("{}", crate::tr!("Portachiavi di sistema non disponibile: {0}", "System keyring not available: {0}", .0))]
    Keyring(String),
    #[error("{}", crate::tr!("Claude Code: {0}", "Claude Code: {0}", .0))]
    ClaudeCode(String),
    #[error("{}", crate::tr!("Lettura dell'immagine non riuscita: {0}", "Could not read the image: {0}", .0))]
    Ocr(String),
    #[error("{}", crate::tr!("Motore AI integrato: {0}", "Built-in AI engine: {0}", .0))]
    Builtin(String),
    #[error("{}", crate::tr!("Download del modello non riuscito: {0}", "Model download failed: {0}", .0))]
    ModelDownload(String),
    #[error("Hugging Face: {0}")]
    Hub(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

fn retry_hint(seconds: Option<u64>) -> String {
    match seconds {
        Some(seconds) => crate::tr!("tra {} secondi", "in {} seconds", seconds),
        None => crate::tr!("più tardi", "later"),
    }
}
