//! Provider AI integrato (ADR 0020): `llama-server` di llama.cpp distribuito
//! con l'app e modelli scaricati su richiesta dal [`catalog`]. Non serve
//! installare Ollama o altri server; i dati non escono dal computer.
//!
//! Il provider non parla direttamente con i modelli: avvia il server adatto
//! ([`server::BuiltinRuntime`]) e gli inoltra le richieste con
//! [`OpenAiCompatible`], lo stesso client usato per i server esterni.

pub mod catalog;
pub mod server;
pub mod store;

use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use catalog::{CatalogModel, ModelRole, find_with_role};
use server::{BuiltinRuntime, ServerLease, ServerSpec, chat_context_tokens};

use super::{
    AiError,
    provider::{AiProvider, ChatMessage, OpenAiCompatible, Provenance},
    settings::AiSettings,
};

/// Nome mostrato come origine delle risposte.
pub const BUILTIN_HOST: &str = "Greatnote (integrato)";

pub struct BuiltinProvider {
    runtime: Arc<BuiltinRuntime>,
    chat_context_tokens: u32,
    /// Modello per le immagini scelto nelle impostazioni: se usa lo stesso
    /// file della chat, la chat usa lo stesso processo (con il proiettore)
    /// invece di caricare il modello due volte.
    vision_model: Option<&'static CatalogModel>,
    timeout: Duration,
}

impl BuiltinProvider {
    pub fn new(runtime: Arc<BuiltinRuntime>, settings: &AiSettings, timeout: Duration) -> Self {
        Self {
            runtime,
            chat_context_tokens: chat_context_tokens(settings.max_context_chars),
            vision_model: find_with_role(&settings.vision_model, ModelRole::Vision),
            timeout,
        }
    }

    fn spec(&self, model_id: &str, role: ModelRole) -> Result<ServerSpec, AiError> {
        let model = find_with_role(model_id, role).ok_or(AiError::NotConfigured(match role {
            ModelRole::Chat => "scegli un modello per la chat tra quelli scaricabili",
            ModelRole::Embedding => "scegli un modello per la ricerca semantica",
            ModelRole::Vision => "scegli un modello per leggere le immagini",
        }))?;
        let model = match self.vision_model {
            Some(vision) if role == ModelRole::Chat && vision.model == model.model => vision,
            _ => model,
        };
        Ok(ServerSpec::for_model(model, self.chat_context_tokens))
    }

    fn client(
        &self,
        spec: &ServerSpec,
        cancel: &AtomicBool,
    ) -> Result<(ServerLease, OpenAiCompatible), AiError> {
        let lease = self.runtime.lease(spec, cancel)?;
        let client = OpenAiCompatible::new(
            lease.endpoint.clone(),
            Some(lease.api_key.clone()),
            self.timeout,
        );
        Ok((lease, client))
    }
}

impl AiProvider for BuiltinProvider {
    fn list_models(&self) -> Result<Vec<String>, AiError> {
        Ok(self
            .runtime
            .store()
            .installed()
            .into_iter()
            .map(|model| model.id.to_owned())
            .collect())
    }

    fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f32,
        cancel: &AtomicBool,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, AiError> {
        let spec = self.spec(model, ModelRole::Chat)?;
        let (_lease, client) = self.client(&spec, cancel)?;
        client.chat(spec.alias, messages, temperature, cancel, on_delta)
    }

    fn embed(
        &self,
        model: &str,
        inputs: &[String],
        cancel: &AtomicBool,
    ) -> Result<Vec<Vec<f32>>, AiError> {
        let spec = self.spec(model, ModelRole::Embedding)?;
        let (_lease, client) = self.client(&spec, cancel)?;
        client.embed(spec.alias, inputs, cancel)
    }

    fn provenance(&self, model: &str) -> Provenance {
        Provenance {
            host: BUILTIN_HOST.to_owned(),
            model: catalog::find(model)
                .map(|entry| entry.name.to_owned())
                .unwrap_or_else(|| model.to_owned()),
            local: true,
        }
    }

    fn read_image(
        &self,
        model: &str,
        instructions: &str,
        request: &str,
        image: &[u8],
        mime: &str,
        cancel: &AtomicBool,
    ) -> Result<String, AiError> {
        let spec = self.spec(model, ModelRole::Vision)?;
        let (_lease, client) = self.client(&spec, cancel)?;
        client.read_image(spec.alias, instructions, request, image, mime, cancel)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::ai::builtin::store::ModelStore;

    fn provider(vision_model: &str) -> BuiltinProvider {
        let runtime = Arc::new(BuiltinRuntime::new(
            None,
            ModelStore::new(PathBuf::from("models")),
        ));
        let settings = AiSettings {
            vision_model: vision_model.to_owned(),
            ..AiSettings::default()
        };
        BuiltinProvider::new(runtime, &settings, Duration::from_secs(30))
    }

    #[test]
    fn chat_shares_the_vision_process_when_the_model_file_is_the_same() {
        let shared = provider("gemma-4-e2b-vision");
        let chat = shared.spec("gemma-4-e2b", ModelRole::Chat).expect("chat");
        let vision = shared
            .spec("gemma-4-e2b-vision", ModelRole::Vision)
            .expect("vision");
        // Stessa configurazione: un solo processo per chat e immagini.
        assert!(chat.projector_file.is_some());
        assert_eq!(chat, vision);

        let separate = provider("qwen3-vl-2b");
        let chat = separate.spec("gemma-4-e2b", ModelRole::Chat).expect("chat");
        assert!(chat.projector_file.is_none());
        assert_eq!(chat.alias, "gemma-4-e2b");
    }

    #[test]
    fn models_must_match_their_role() {
        let provider = provider("");
        assert!(matches!(
            provider.spec("bge-m3", ModelRole::Chat),
            Err(AiError::NotConfigured(_))
        ));
        assert!(matches!(
            provider.spec("qwen3:8b", ModelRole::Chat),
            Err(AiError::NotConfigured(_))
        ));
        let provenance = provider.provenance("bge-m3");
        assert!(provenance.local);
        assert_eq!(provenance.model, "BGE-M3");
        assert!(matches!(
            provider.chat(
                "gemma-4-e2b",
                &[],
                0.3,
                &AtomicBool::new(false),
                &mut |_| {}
            ),
            Err(AiError::NotConfigured(_))
        ));
    }
}
