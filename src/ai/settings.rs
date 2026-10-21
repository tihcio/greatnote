//! Impostazioni AI: salvate nella tabella `settings` (quindi nei backup), mai
//! con segreti. La chiave API, se serve, sta nel portachiavi di sistema ed è
//! gestita dalla shell desktop.

use std::net::{Ipv4Addr, Ipv6Addr};

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use super::{
    AiError,
    builtin::catalog::{ModelRole, is_valid_model_id},
};
use crate::storage::{Database, StorageError};

pub const AI_SETTINGS_KEY: &str = "ai.settings";
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";
const MAX_MODEL_NAME: usize = 200;
const MAX_URL_LENGTH: usize = 500;
const MAX_EXCLUDED_NOTEBOOKS: usize = 50;
pub const CONTEXT_CHARS_RANGE: (u32, u32) = (2_000, 60_000);
pub const TIMEOUT_SECONDS_RANGE: (u32, u32) = (10, 600);
pub const VISION_SIDE_RANGE: (u32, u32) = (384, 4096);

/// Tipo di provider: server con API compatibile OpenAI, la CLI di Claude
/// Code installata sul computer (con il suo account) oppure il motore
/// integrato.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    #[default]
    OpenaiCompatible,
    ClaudeCode,
    /// Motore llama.cpp distribuito con l'app e modelli scaricati dal
    /// catalogo (ADR 0020): nessun server da installare, tutto locale.
    Builtin,
}

/// Motore per leggere il testo delle immagini allegate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OcrEngine {
    /// Modello AI con visione del provider configurato.
    #[default]
    Ai,
    /// Tesseract installato sul computer (locale, nessun invio).
    Tesseract,
}

/// Server a cui Claude Code invia i dati (per l'interfaccia e la cronologia).
pub const CLAUDE_CODE_HOST: &str = "Anthropic (Claude Code)";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    /// Interruttore generale: falso finché l'utente non attiva l'AI.
    pub enabled: bool,
    pub provider: ProviderKind,
    /// Percorso dell'eseguibile `claude`; vuoto: ricerca automatica.
    pub claude_path: String,
    /// Indirizzo base dell'API compatibile OpenAI (es. `http://localhost:11434/v1`).
    pub base_url: String,
    pub chat_model: String,
    /// Vuoto: niente ricerca semantica, il recupero usa la ricerca testuale.
    pub embedding_model: String,
    /// Aggiorna l'indice semantico in background quando le note cambiano.
    pub auto_index: bool,
    /// Conserva sul dispositivo la cronologia delle richieste AI.
    pub keep_history: bool,
    pub ocr_engine: OcrEngine,
    /// Legge il testo delle immagini appena vengono allegate.
    pub ocr_on_attach: bool,
    /// Modello con visione per i server compatibili OpenAI; vuoto: quello
    /// della chat.
    pub vision_model: String,
    /// Lato lungo massimo (pixel) delle immagini inviate ai modelli con
    /// visione: meno pixel, meno token di contesto.
    pub vision_max_side: u32,
    /// La chiave è nel portachiavi; qui si ricorda solo se va inviata.
    pub use_api_key: bool,
    /// Consenso esplicito a inviare testo a un server non locale.
    pub remote_consent: bool,
    /// Mostra cosa verrà inviato a un server remoto e chiede conferma prima
    /// di ogni richiesta. Si disattiva con "Non chiedermelo più".
    pub confirm_remote_sends: bool,
    /// Le note di questi taccuini non vengono mai inviate né indicizzate.
    pub excluded_notebook_ids: Vec<String>,
    /// La chat usa anche attività, eventi del calendario e promemoria come
    /// contesto (quelli collegati a note escluse restano esclusi).
    pub planner_context: bool,
    /// Limite del testo delle note inviato come contesto in una richiesta.
    pub max_context_chars: u32,
    pub timeout_seconds: u32,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: ProviderKind::OpenaiCompatible,
            claude_path: String::new(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            chat_model: String::new(),
            embedding_model: String::new(),
            auto_index: false,
            keep_history: true,
            ocr_engine: OcrEngine::Ai,
            ocr_on_attach: false,
            vision_model: String::new(),
            vision_max_side: 1024,
            use_api_key: false,
            remote_consent: false,
            confirm_remote_sends: true,
            excluded_notebook_ids: Vec::new(),
            planner_context: true,
            max_context_chars: 12_000,
            timeout_seconds: 120,
        }
    }
}

impl AiSettings {
    /// Controlla e normalizza i valori ricevuti dall'interfaccia.
    pub fn validated(mut self) -> Result<Self, AiError> {
        self.base_url = self.base_url.trim().trim_end_matches('/').to_owned();
        Endpoint::parse(&self.base_url)?;
        self.chat_model = validate_model(&self.chat_model)?;
        self.embedding_model = validate_model(&self.embedding_model)?;
        self.vision_model = validate_model(&self.vision_model)?;
        if !(VISION_SIDE_RANGE.0..=VISION_SIDE_RANGE.1).contains(&self.vision_max_side) {
            return Err(AiError::InvalidSettings(
                "risoluzione delle immagini fuori dall'intervallo consentito",
            ));
        }
        self.claude_path = self.claude_path.trim().to_owned();
        if !self.claude_path.is_empty()
            && (self.claude_path.len() > MAX_URL_LENGTH
                || self.claude_path.chars().any(char::is_control)
                || !std::path::Path::new(&self.claude_path).is_absolute())
        {
            return Err(AiError::InvalidSettings(
                "il percorso di Claude Code deve essere assoluto (es. /home/utente/.local/bin/claude)",
            ));
        }
        if self.provider == ProviderKind::ClaudeCode {
            // Claude Code non fornisce embedding: niente indice semantico.
            self.embedding_model.clear();
            self.auto_index = false;
        }
        if self.provider == ProviderKind::Builtin {
            // Solo modelli del catalogo o di Hugging Face, ciascuno nel suo
            // ruolo; la presenza dei file si controlla quando vengono usati.
            for (model, role) in [
                (&self.chat_model, ModelRole::Chat),
                (&self.embedding_model, ModelRole::Embedding),
                (&self.vision_model, ModelRole::Vision),
            ] {
                if !model.is_empty() && !is_valid_model_id(model, role) {
                    return Err(AiError::InvalidSettings(
                        "modello non presente tra quelli del motore integrato",
                    ));
                }
            }
        }
        if !(CONTEXT_CHARS_RANGE.0..=CONTEXT_CHARS_RANGE.1).contains(&self.max_context_chars) {
            return Err(AiError::InvalidSettings(
                "contesto massimo fuori dall'intervallo consentito",
            ));
        }
        if !(TIMEOUT_SECONDS_RANGE.0..=TIMEOUT_SECONDS_RANGE.1).contains(&self.timeout_seconds) {
            return Err(AiError::InvalidSettings(
                "tempo massimo fuori dall'intervallo consentito",
            ));
        }
        let mut excluded: Vec<String> = Vec::new();
        for id in self.excluded_notebook_ids {
            let id = id.trim().to_owned();
            if id.is_empty() || id.len() > 64 || !id.chars().all(is_id_char) {
                return Err(AiError::InvalidSettings("taccuino escluso non valido"));
            }
            if !excluded.contains(&id) {
                excluded.push(id);
            }
        }
        if excluded.len() > MAX_EXCLUDED_NOTEBOOKS {
            return Err(AiError::InvalidSettings("troppi taccuini esclusi"));
        }
        self.excluded_notebook_ids = excluded;
        Ok(self)
    }

    pub fn endpoint(&self) -> Result<Endpoint, AiError> {
        match self.provider {
            ProviderKind::OpenaiCompatible => Endpoint::parse(&self.base_url),
            // I dati escono dal computer verso Anthropic: è un server remoto.
            ProviderKind::ClaudeCode => Ok(Endpoint {
                base_url: String::new(),
                host: CLAUDE_CODE_HOST.to_owned(),
                https: true,
            }),
            // Processo su questo computer, raggiunto solo su 127.0.0.1.
            ProviderKind::Builtin => Ok(Endpoint {
                base_url: String::new(),
                host: "localhost".to_owned(),
                https: false,
            }),
        }
    }

    /// Verifica che l'AI sia attiva e che i dati possano essere inviati al
    /// server configurato.
    pub fn ensure_usable(&self) -> Result<Endpoint, AiError> {
        if !self.enabled {
            return Err(AiError::Disabled);
        }
        let endpoint = self.endpoint()?;
        if !endpoint.is_local() && !self.remote_consent {
            return Err(AiError::RemoteConsentRequired(endpoint.host.clone()));
        }
        Ok(endpoint)
    }

    pub fn ensure_chat_model(&self) -> Result<&str, AiError> {
        // Con Claude Code il modello vuoto è quello predefinito dell'account.
        if self.chat_model.is_empty() && self.provider != ProviderKind::ClaudeCode {
            return Err(AiError::NotConfigured("scegli un modello per la chat"));
        }
        Ok(&self.chat_model)
    }

    pub fn ensure_embedding_model(&self) -> Result<&str, AiError> {
        if self.embedding_model.is_empty() {
            return Err(AiError::NotConfigured(
                "scegli un modello di embedding per la ricerca semantica",
            ));
        }
        Ok(&self.embedding_model)
    }

    pub fn is_excluded(&self, notebook_id: Option<&str>) -> bool {
        notebook_id.is_some_and(|id| self.excluded_notebook_ids.iter().any(|item| item == id))
    }

    /// Elenco dei taccuini esclusi in JSON, per i filtri SQL con `json_each`.
    pub(crate) fn excluded_json(&self) -> String {
        serde_json::to_string(&self.excluded_notebook_ids).unwrap_or_else(|_| "[]".to_owned())
    }
}

fn is_id_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':')
}

fn validate_model(name: &str) -> Result<String, AiError> {
    let name = name.trim();
    if name.chars().count() > MAX_MODEL_NAME || name.chars().any(char::is_control) {
        return Err(AiError::InvalidSettings("nome del modello non valido"));
    }
    Ok(name.to_owned())
}

/// Indirizzo del server AI, scomposto e verificato. Solo `http` e `https`,
/// senza credenziali, query o frammenti nell'URL. Un server non locale deve
/// usare `https`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub base_url: String,
    pub host: String,
    pub https: bool,
}

impl Endpoint {
    pub fn parse(url: &str) -> Result<Self, AiError> {
        const INVALID: AiError = AiError::InvalidSettings(
            "indirizzo del server non valido (esempio: http://localhost:11434/v1)",
        );
        let url = url.trim().trim_end_matches('/');
        if url.len() > MAX_URL_LENGTH || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(INVALID);
        }
        let lower = url.to_ascii_lowercase();
        let (https, rest) = if let Some(rest) = lower.strip_prefix("https://") {
            (true, &url[url.len() - rest.len()..])
        } else if let Some(rest) = lower.strip_prefix("http://") {
            (false, &url[url.len() - rest.len()..])
        } else {
            return Err(INVALID);
        };
        if rest.contains(['?', '#', '@', '\\']) {
            return Err(INVALID);
        }
        let authority = rest.split('/').next().unwrap_or_default();
        let host = if let Some(bracketed) = authority.strip_prefix('[') {
            let (address, after) = bracketed.split_once(']').ok_or(INVALID)?;
            if !(after.is_empty() || valid_port(after.strip_prefix(':').ok_or(INVALID)?)) {
                return Err(INVALID);
            }
            address.parse::<Ipv6Addr>().map_err(|_| INVALID)?;
            address.to_owned()
        } else {
            let (host, port) = match authority.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            };
            if port.is_some_and(|port| !valid_port(port)) {
                return Err(INVALID);
            }
            if host.is_empty()
                || !host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            {
                return Err(INVALID);
            }
            host.to_ascii_lowercase()
        };
        let endpoint = Self {
            base_url: url.to_owned(),
            host,
            https,
        };
        if !endpoint.is_local() && !https {
            return Err(AiError::InvalidSettings(
                "un server che non è sul tuo computer deve usare https",
            ));
        }
        Ok(endpoint)
    }

    /// Vero se il server è su questo computer (loopback): i dati non escono
    /// dalla macchina.
    pub fn is_local(&self) -> bool {
        self.host == "localhost"
            || self.host.ends_with(".localhost")
            || self
                .host
                .parse::<Ipv4Addr>()
                .is_ok_and(|address| address.is_loopback())
            || self
                .host
                .parse::<Ipv6Addr>()
                .is_ok_and(|address| address.is_loopback())
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.len() <= 5 && port.parse::<u16>().is_ok_and(|port| port > 0)
}

impl Database {
    pub fn ai_settings(&self) -> Result<AiSettings, StorageError> {
        let connection = self.connection()?;
        let stored: Option<String> = connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [AI_SETTINGS_KEY],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match stored {
            // Valori illeggibili o non più validi tornano ai predefiniti, che
            // tengono l'AI disattivata.
            Some(value) => serde_json::from_str::<AiSettings>(&value)
                .ok()
                .and_then(|settings| settings.validated().ok())
                .unwrap_or_default(),
            None => AiSettings::default(),
        })
    }

    pub fn set_ai_settings(&self, settings: AiSettings) -> Result<AiSettings, AiError> {
        let settings = settings.validated()?;
        let value = serde_json::to_string(&settings).map_err(StorageError::from)?;
        if value.len() > 4096 {
            return Err(AiError::InvalidSettings("impostazioni troppo estese"));
        }
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                [AI_SETTINGS_KEY, value.as_str()],
            )
            .map_err(StorageError::from)?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_ai_disabled_and_local() {
        let database = Database::in_memory().expect("database");
        let settings = database.ai_settings().expect("settings");
        assert!(!settings.enabled);
        assert!(settings.endpoint().expect("endpoint").is_local());
        assert!(matches!(settings.ensure_usable(), Err(AiError::Disabled)));
    }

    #[test]
    fn endpoints_are_parsed_strictly() {
        for local in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080/v1/",
            "http://[::1]:1234/v1",
            "HTTP://LocalHost/v1",
        ] {
            assert!(Endpoint::parse(local).expect(local).is_local(), "{local}");
        }
        let remote = Endpoint::parse("https://api.example.com/v1").expect("remote");
        assert!(!remote.is_local());
        assert_eq!(remote.url("/models"), "https://api.example.com/v1/models");
        for invalid in [
            "ftp://localhost/v1",
            "http://api.example.com/v1",
            "https://user:pw@example.com/v1",
            "https://example.com/v1?x=1",
            "https://exa mple.com",
            "http://localhost:99999",
            "http://[zz]/v1",
            "file:///etc/passwd",
            "",
        ] {
            assert!(Endpoint::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn claude_code_is_remote_without_embeddings() {
        let settings = AiSettings {
            enabled: true,
            provider: ProviderKind::ClaudeCode,
            embedding_model: "nomic-embed-text".to_owned(),
            auto_index: true,
            ..AiSettings::default()
        }
        .validated()
        .expect("valid");
        assert!(settings.embedding_model.is_empty());
        assert!(!settings.auto_index);
        assert!(settings.ensure_chat_model().is_ok(), "modello predefinito");
        assert!(matches!(
            settings.ensure_usable(),
            Err(AiError::RemoteConsentRequired(host)) if host == CLAUDE_CODE_HOST
        ));
        let consented = AiSettings {
            remote_consent: true,
            ..settings
        };
        assert!(!consented.ensure_usable().expect("usable").is_local());
        let relative = AiSettings {
            claude_path: "bin/claude".to_owned(),
            ..consented
        };
        assert!(relative.validated().is_err());
        let stored: AiSettings =
            serde_json::from_str(r#"{"enabled":true,"base_url":"http://localhost:11434/v1"}"#)
                .expect("old settings");
        assert_eq!(stored.provider, ProviderKind::OpenaiCompatible);
    }

    #[test]
    fn builtin_provider_is_local_and_accepts_only_catalog_models() {
        let settings = AiSettings {
            enabled: true,
            provider: ProviderKind::Builtin,
            chat_model: "gemma-4-e2b".to_owned(),
            embedding_model: "bge-m3".to_owned(),
            vision_model: "qwen3-vl-2b".to_owned(),
            ..AiSettings::default()
        }
        .validated()
        .expect("valid");
        assert!(settings.ensure_usable().expect("usable").is_local());
        let stored: AiSettings = serde_json::from_str(r#"{"provider":"builtin"}"#).expect("stored");
        assert_eq!(stored.provider, ProviderKind::Builtin);
        let hub = AiSettings {
            chat_model: "hf:chat:unsloth/gemma-4-12b-it-GGUF:gemma-4-12b-it-Q4_K_M".to_owned(),
            ..settings.clone()
        };
        assert!(
            hub.validated().is_ok(),
            "modello di Hugging Face per la chat"
        );
        for invalid in [
            AiSettings {
                chat_model: "hf:embedding:unsloth/bge-GGUF:bge-Q8_0".to_owned(),
                ..settings.clone()
            },
            AiSettings {
                chat_model: "qwen3:8b".to_owned(),
                ..settings.clone()
            },
            AiSettings {
                embedding_model: "gemma-4-e2b".to_owned(),
                ..settings.clone()
            },
            AiSettings {
                vision_model: "bge-m3".to_owned(),
                ..settings.clone()
            },
        ] {
            assert!(invalid.validated().is_err());
        }
    }

    #[test]
    fn remote_servers_need_consent_and_settings_persist() {
        let database = Database::in_memory().expect("database");
        let settings = AiSettings {
            enabled: true,
            base_url: "https://api.example.com/v1/".to_owned(),
            chat_model: " modello ".to_owned(),
            excluded_notebook_ids: vec!["nb-1".to_owned(), "nb-1".to_owned()],
            ..AiSettings::default()
        };
        let saved = database.set_ai_settings(settings).expect("saved");
        assert_eq!(saved.base_url, "https://api.example.com/v1");
        assert_eq!(saved.chat_model, "modello");
        assert_eq!(saved.excluded_notebook_ids, vec!["nb-1"]);
        assert!(matches!(
            saved.ensure_usable(),
            Err(AiError::RemoteConsentRequired(host)) if host == "api.example.com"
        ));
        let consented = database
            .set_ai_settings(AiSettings {
                remote_consent: true,
                ..saved
            })
            .expect("consent");
        assert!(consented.ensure_usable().is_ok());
        assert_eq!(database.ai_settings().expect("reload"), consented);
        assert!(consented.is_excluded(Some("nb-1")));
        assert!(!consented.is_excluded(None));

        let invalid = AiSettings {
            max_context_chars: 10,
            ..AiSettings::default()
        };
        assert!(database.set_ai_settings(invalid).is_err());
    }
}
