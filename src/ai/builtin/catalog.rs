//! Modelli del provider integrato (ADR 0020, ADR 0021).
//!
//! Due origini:
//! - il **catalogo curato**, incluso nell'app e provato con la versione di
//!   llama.cpp distribuita (`scripts/fetch-llama.ps1`);
//! - i modelli **cercati su Hugging Face** ([`super::hub`]) e registrati nella
//!   cartella dei modelli al momento del download.
//!
//! In entrambi i casi ogni file è fissato a una revisione precisa del
//! repository e verificato con SHA-256 dopo il download: un file diverso da
//! quello previsto viene scartato.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// Prefisso degli id dei modelli scaricati da Hugging Face.
pub const HUB_ID_PREFIX: &str = "hf:";
/// Lunghezza massima di un id (stesso limite dei nomi dei modelli nelle
/// impostazioni).
pub const MAX_MODEL_ID: usize = 200;
/// Lunghezza massima del nome locale di un file.
const MAX_FILE_NAME: usize = 160;

/// Uso del modello nell'app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelRole {
    /// Chat, modifiche e suggerimenti sulle note.
    Chat,
    /// Embedding per la ricerca semantica e il contesto della chat.
    Embedding,
    /// Lettura del testo nelle immagini.
    Vision,
}

impl ModelRole {
    pub const ALL: [ModelRole; 3] = [ModelRole::Chat, ModelRole::Embedding, ModelRole::Vision];

    pub fn as_str(self) -> &'static str {
        match self {
            ModelRole::Chat => "chat",
            ModelRole::Embedding => "embedding",
            ModelRole::Vision => "vision",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.as_str() == value)
    }
}

/// File scaricabile: nome locale, indirizzo fisso, dimensione e hash attesi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFile {
    pub file_name: String,
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
}

impl ModelFile {
    /// Nome locale sicuro, indirizzo Hugging Face con revisione fissa e hash
    /// ben formato. I file letti dal disco o arrivati dalla rete passano da
    /// qui prima di essere usati.
    pub fn is_valid(&self) -> bool {
        is_safe_file_name(&self.file_name)
            && self.size_bytes > 0
            && self.sha256.len() == 64
            && self
                .sha256
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'))
            && self.url.starts_with("https://huggingface.co/")
            && !self.url.contains("/resolve/main/")
            && self.url.len() <= 600
            && !self
                .url
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
    }
}

/// Origine di un modello.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ModelSource {
    /// Catalogo incluso nell'app, provato con il motore distribuito.
    Curated,
    /// Scelto dall'utente tra i risultati di Hugging Face.
    HuggingFace { repo: String, revision: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogModel {
    /// Identificativo salvato nelle impostazioni.
    pub id: String,
    pub role: ModelRole,
    pub name: String,
    pub license: String,
    /// Memoria indicativa necessaria (GB), mostrata all'utente.
    pub memory_gb: f32,
    /// Proposto per primo nella sua categoria (solo catalogo curato).
    pub recommended: bool,
    /// File principale; per i modelli divisi in più parti è la prima.
    pub model: ModelFile,
    /// Parti successive di un modello diviso (`-00002-of-00003.gguf`...).
    #[serde(default)]
    pub parts: Vec<ModelFile>,
    /// Proiettore multimodale (solo per i modelli con visione).
    pub projector: Option<ModelFile>,
    pub source: ModelSource,
}

impl CatalogModel {
    pub fn files(&self) -> impl Iterator<Item = &ModelFile> {
        std::iter::once(&self.model)
            .chain(self.parts.iter())
            .chain(self.projector.iter())
    }

    pub fn size_bytes(&self) -> u64 {
        self.files().map(|file| file.size_bytes).sum()
    }

    pub fn is_curated(&self) -> bool {
        self.source == ModelSource::Curated
    }

    /// Coerenza di un modello letto dal disco o costruito dai dati di
    /// Hugging Face: id del ruolo giusto, file validi e proiettore solo per
    /// la visione.
    pub fn is_valid(&self) -> bool {
        let id_ok = match &self.source {
            ModelSource::Curated => true,
            ModelSource::HuggingFace { repo, revision } => {
                hub_id_role(&self.id) == Some(self.role)
                    && is_valid_repo_id(repo)
                    && is_valid_revision(revision)
            }
        };
        id_ok
            && self.id.len() <= MAX_MODEL_ID
            && !self.name.trim().is_empty()
            && self.name.chars().count() <= 200
            && self.license.chars().count() <= 60
            && self.parts.len() <= 64
            && self.projector.is_some() == (self.role == ModelRole::Vision)
            && self.files().all(ModelFile::is_valid)
    }
}

/// Nome locale accettato per un file di modello: solo caratteri sicuri su
/// ogni sistema, estensione `.gguf`, nessun separatore di percorso.
pub fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FILE_NAME
        && !name.starts_with('.')
        && name.ends_with(".gguf")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Repository Hugging Face `autore/nome`, con i caratteri ammessi da Hugging
/// Face e senza `..`: entra in un percorso dell'URL.
pub fn is_valid_repo_id(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    [owner, name].iter().all(|part| {
        !part.is_empty()
            && part.len() <= 96
            && !part.starts_with(['.', '-'])
            && !part.contains("..")
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    })
}

/// Hash del commit (40 caratteri esadecimali): mai un ramo mobile.
pub fn is_valid_revision(revision: &str) -> bool {
    revision.len() == 40
        && revision
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'))
}

/// Id di un modello di Hugging Face: `hf:<ruolo>:<autore/repo>:<variante>`.
pub fn hub_model_id(role: ModelRole, repo: &str, variant: &str) -> String {
    format!("{HUB_ID_PREFIX}{}:{repo}:{variant}", role.as_str())
}

/// Ruolo di un id di Hugging Face ben formato.
pub fn hub_id_role(id: &str) -> Option<ModelRole> {
    let rest = id.strip_prefix(HUB_ID_PREFIX)?;
    let mut fields = rest.splitn(3, ':');
    let role = ModelRole::parse(fields.next()?)?;
    let repo = fields.next()?;
    let variant = fields.next()?;
    let variant_ok = !variant.is_empty()
        && variant.len() <= 120
        && variant
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    (id.len() <= MAX_MODEL_ID && is_valid_repo_id(repo) && variant_ok).then_some(role)
}

/// Un id accettabile per il ruolo: del catalogo curato o di Hugging Face.
/// La presenza effettiva del modello si controlla quando viene usato.
pub fn is_valid_model_id(id: &str, role: ModelRole) -> bool {
    find_with_role(id, role).is_some() || hub_id_role(id) == Some(role)
}

fn curated_file(file_name: &str, repo_path: &str, size_bytes: u64, sha256: &str) -> ModelFile {
    ModelFile {
        file_name: file_name.to_owned(),
        url: format!("https://huggingface.co/{repo_path}/{file_name}"),
        size_bytes,
        sha256: sha256.to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn curated_model(
    id: &str,
    role: ModelRole,
    name: &str,
    license: &str,
    memory_gb: f32,
    recommended: bool,
    model: ModelFile,
    projector: Option<ModelFile>,
) -> CatalogModel {
    CatalogModel {
        id: id.to_owned(),
        role,
        name: name.to_owned(),
        license: license.to_owned(),
        memory_gb,
        recommended,
        model,
        parts: Vec::new(),
        projector,
        source: ModelSource::Curated,
    }
}

/// Modelli proposti, nell'ordine in cui vengono mostrati.
static CURATED: LazyLock<Vec<CatalogModel>> = LazyLock::new(|| {
    let qwen35_08b = curated_file(
        "Qwen3.5-0.8B-Q8_0.gguf",
        "ggml-org/Qwen3.5-0.8B-GGUF/resolve/807dfd9a6fadbf703207aac3bfb0b2b3040d077c",
        833_592_096,
        "37ae482d336108d23516fa35e8e0c4126688d81018b87178a18d752a1357814f",
    );
    let gemma4_e2b = curated_file(
        "gemma-4-E2B-it-Q4_0.gguf",
        "ggml-org/gemma-4-E2B-it-GGUF/resolve/6938c58e698502037401a5f19f6905f2fb2f7dee",
        2_841_481_184,
        "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52",
    );
    let gemma4_e2b_projector = curated_file(
        "mmproj-gemma-4-E2B-it-Q8_0.gguf",
        "ggml-org/gemma-4-E2B-it-GGUF/resolve/6938c58e698502037401a5f19f6905f2fb2f7dee",
        557_368_064,
        "9406f99c16d68cda4f1f0552192dcc99021ea1fc6d2fd50b1dc3ccf30d04b292",
    );
    let gemma4_e4b = curated_file(
        "gemma-4-E4B-it-Q4_0.gguf",
        "ggml-org/gemma-4-E4B-it-GGUF/resolve/a69f018d7678a2b9f5e705f8903d6fb29b9b25dc",
        4_590_807_392,
        "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2",
    );
    let bge_m3 = curated_file(
        "bge-m3-q8_0.gguf",
        "ggml-org/bge-m3-Q8_0-GGUF/resolve/9eba04c5d75ba5a1595e45de734d36bef4e5cb98",
        634_553_760,
        "aa473d51f451a22f0fcf39ba3330c14bed38a385712b1113440f69df4047a173",
    );
    let nomic_embed_v2 = curated_file(
        "nomic-embed-text-v2-moe-q8_0.gguf",
        "ggml-org/Nomic-Embed-Text-V2-GGUF/resolve/498da4a128ed12a423efb6f9b0242dbac80209bf",
        512_225_120,
        "36c5817bc25f379e62021f49efde05b10ed3b0c93ab8059c43173a7a5de73565",
    );
    let qwen3_vl_2b = curated_file(
        "Qwen3-VL-2B-Instruct-Q8_0.gguf",
        "ggml-org/Qwen3-VL-2B-Instruct-GGUF/resolve/ea6a11058182570be6436b9a2e4ee7f7b49f908d",
        1_834_427_296,
        "b7802e29f71a9e5b5e3f83f613df898a2204342dcea71a231ea501d481813c39",
    );
    let qwen3_vl_2b_projector = curated_file(
        "mmproj-Qwen3-VL-2B-Instruct-Q8_0.gguf",
        "ggml-org/Qwen3-VL-2B-Instruct-GGUF/resolve/ea6a11058182570be6436b9a2e4ee7f7b49f908d",
        445_053_056,
        "69066c8f279ec85ff48ab4059f6ebba0d2932ca57667f2bbdac7d9805bca9e7b",
    );
    vec![
        curated_model(
            "gemma-4-e2b",
            ModelRole::Chat,
            "Gemma 4 E2B",
            "Apache 2.0",
            4.0,
            true,
            gemma4_e2b.clone(),
            None,
        ),
        curated_model(
            "qwen3.5-0.8b",
            ModelRole::Chat,
            "Qwen3.5 0.8B",
            "Apache 2.0",
            1.5,
            false,
            qwen35_08b,
            None,
        ),
        curated_model(
            "gemma-4-e4b",
            ModelRole::Chat,
            "Gemma 4 E4B",
            "Apache 2.0",
            6.0,
            false,
            gemma4_e4b,
            None,
        ),
        curated_model(
            "bge-m3",
            ModelRole::Embedding,
            "BGE-M3",
            "MIT",
            1.0,
            true,
            bge_m3,
            None,
        ),
        curated_model(
            "nomic-embed-text-v2",
            ModelRole::Embedding,
            "Nomic Embed Text v2",
            "Apache 2.0",
            1.0,
            false,
            nomic_embed_v2,
            None,
        ),
        curated_model(
            "qwen3-vl-2b",
            ModelRole::Vision,
            "Qwen3-VL 2B",
            "Apache 2.0",
            3.5,
            true,
            qwen3_vl_2b,
            Some(qwen3_vl_2b_projector),
        ),
        // Stesso file della chat: se è già scaricato serve solo il proiettore.
        curated_model(
            "gemma-4-e2b-vision",
            ModelRole::Vision,
            "Gemma 4 E2B",
            "Apache 2.0",
            4.5,
            false,
            gemma4_e2b,
            Some(gemma4_e2b_projector),
        ),
    ]
});

/// Catalogo curato incluso nell'app.
pub fn curated() -> &'static [CatalogModel] {
    &CURATED
}

/// Modello del catalogo curato.
pub fn find(id: &str) -> Option<&'static CatalogModel> {
    curated().iter().find(|model| model.id == id)
}

/// Modello del catalogo curato con il ruolo richiesto.
pub fn find_with_role(id: &str, role: ModelRole) -> Option<&'static CatalogModel> {
    find(id).filter(|model| model.role == role)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn catalog_entries_are_consistent() {
        let mut ids = Vec::new();
        let mut files: HashMap<&str, &ModelFile> = HashMap::new();
        for model in curated() {
            assert!(!ids.contains(&model.id), "id duplicato {}", model.id);
            ids.push(model.id.clone());
            assert!(model.is_valid(), "{}", model.id);
            assert!(model.is_curated());
            assert!(hub_id_role(&model.id).is_none());
            for file in model.files() {
                assert!(file.url.ends_with(&file.file_name));
                // Lo stesso nome locale indica sempre lo stesso file.
                if let Some(previous) = files.insert(&file.file_name, file) {
                    assert_eq!(previous, file);
                }
            }
        }
        for role in ModelRole::ALL {
            let recommended = curated()
                .iter()
                .filter(|model| model.role == role && model.recommended)
                .count();
            assert_eq!(recommended, 1, "{role:?}");
        }
        assert!(find_with_role("bge-m3", ModelRole::Embedding).is_some());
        assert!(find_with_role("bge-m3", ModelRole::Chat).is_none());
        assert!(find("sconosciuto").is_none());
    }

    #[test]
    fn hub_ids_carry_their_role_and_reject_unsafe_parts() {
        let id = hub_model_id(
            ModelRole::Chat,
            "unsloth/gemma-4-12b-it-GGUF",
            "gemma-4-12b-it-Q4_K_M",
        );
        assert_eq!(
            id,
            "hf:chat:unsloth/gemma-4-12b-it-GGUF:gemma-4-12b-it-Q4_K_M"
        );
        assert_eq!(hub_id_role(&id), Some(ModelRole::Chat));
        assert!(is_valid_model_id(&id, ModelRole::Chat));
        assert!(!is_valid_model_id(&id, ModelRole::Vision));
        assert!(is_valid_model_id("bge-m3", ModelRole::Embedding));
        for invalid in [
            "hf:chat:../etc:x",
            "hf:chat:autore/repo/altro:x",
            "hf:chat:autore/repo:var iante",
            "hf:sconosciuto:autore/repo:x",
            "hf:chat:autore/repo:",
            "hf:chat:autore:x",
            "qwen3:8b",
        ] {
            assert!(hub_id_role(invalid).is_none(), "{invalid}");
        }
        assert!(is_valid_repo_id("ggml-org/bge-m3-Q8_0-GGUF"));
        for invalid in [
            "", "solo", "a/b/c", "../x", "a/..b", ".a/b", "a/b c", "a/b?x",
        ] {
            assert!(!is_valid_repo_id(invalid), "{invalid}");
        }
        assert!(is_valid_revision(
            "6938c58e698502037401a5f19f6905f2fb2f7dee"
        ));
        assert!(!is_valid_revision("main"));
        assert!(is_safe_file_name(
            "hf-0123456789ab-Q4_K_M_model-00001-of-00002.gguf"
        ));
        for invalid in [
            "../x.gguf",
            "a/b.gguf",
            "a\\b.gguf",
            ".x.gguf",
            "x.bin",
            "con figura.gguf",
        ] {
            assert!(!is_safe_file_name(invalid), "{invalid}");
        }
    }

    #[test]
    fn stored_models_round_trip_and_invalid_ones_are_detected() {
        let model = find("qwen3-vl-2b").expect("model").clone();
        let json = serde_json::to_string(&model).expect("json");
        let parsed: CatalogModel = serde_json::from_str(&json).expect("parsed");
        assert_eq!(parsed, model);
        let mut without_projector = model.clone();
        without_projector.projector = None;
        assert!(!without_projector.is_valid());
        let mut moving_branch = model.clone();
        moving_branch.model.url = "https://huggingface.co/a/b/resolve/main/x.gguf".to_owned();
        assert!(!moving_branch.is_valid());
        let mut wrong_id = model;
        wrong_id.source = ModelSource::HuggingFace {
            repo: "a/b".to_owned(),
            revision: "6938c58e698502037401a5f19f6905f2fb2f7dee".to_owned(),
        };
        assert!(
            !wrong_id.is_valid(),
            "un id di Hugging Face deve indicare il ruolo"
        );
    }
}
