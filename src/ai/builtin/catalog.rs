//! Catalogo dei modelli scaricabili per il provider integrato (ADR 0020).
//!
//! Ogni file è fissato a una revisione precisa del repository su Hugging
//! Face e verificato con SHA-256 dopo il download: un file diverso da quello
//! previsto viene scartato. Sono inclusi solo modelli con licenza aperta
//! (Apache 2.0 o MIT) e architetture supportate dalla versione di llama.cpp
//! distribuita con l'app (`scripts/fetch-llama.ps1`).

use serde::Serialize;

/// Uso del modello nell'app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelRole {
    /// Chat, modifiche e suggerimenti sulle note.
    Chat,
    /// Embedding per la ricerca semantica e il contesto della chat.
    Embedding,
    /// Lettura del testo nelle immagini.
    Vision,
}

/// File scaricabile: nome locale, indirizzo fisso, dimensione e hash attesi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    pub file_name: &'static str,
    pub url: &'static str,
    pub size_bytes: u64,
    pub sha256: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CatalogModel {
    /// Identificativo salvato nelle impostazioni.
    pub id: &'static str,
    pub role: ModelRole,
    pub name: &'static str,
    pub license: &'static str,
    /// Memoria indicativa necessaria (GB), mostrata all'utente.
    pub memory_gb: f32,
    /// Proposto per primo nella sua categoria.
    pub recommended: bool,
    pub model: ModelFile,
    /// Proiettore multimodale (solo per i modelli con visione).
    pub projector: Option<ModelFile>,
}

impl CatalogModel {
    pub fn files(&self) -> impl Iterator<Item = &ModelFile> {
        std::iter::once(&self.model).chain(self.projector.iter())
    }

    pub fn size_bytes(&self) -> u64 {
        self.files().map(|file| file.size_bytes).sum()
    }
}

const QWEN35_08B: ModelFile = ModelFile {
    file_name: "Qwen3.5-0.8B-Q8_0.gguf",
    url: "https://huggingface.co/ggml-org/Qwen3.5-0.8B-GGUF/resolve/807dfd9a6fadbf703207aac3bfb0b2b3040d077c/Qwen3.5-0.8B-Q8_0.gguf",
    size_bytes: 833_592_096,
    sha256: "37ae482d336108d23516fa35e8e0c4126688d81018b87178a18d752a1357814f",
};

const GEMMA4_E2B: ModelFile = ModelFile {
    file_name: "gemma-4-E2B-it-Q4_0.gguf",
    url: "https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF/resolve/6938c58e698502037401a5f19f6905f2fb2f7dee/gemma-4-E2B-it-Q4_0.gguf",
    size_bytes: 2_841_481_184,
    sha256: "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52",
};

const GEMMA4_E2B_PROJECTOR: ModelFile = ModelFile {
    file_name: "mmproj-gemma-4-E2B-it-Q8_0.gguf",
    url: "https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF/resolve/6938c58e698502037401a5f19f6905f2fb2f7dee/mmproj-gemma-4-E2B-it-Q8_0.gguf",
    size_bytes: 557_368_064,
    sha256: "9406f99c16d68cda4f1f0552192dcc99021ea1fc6d2fd50b1dc3ccf30d04b292",
};

const GEMMA4_E4B: ModelFile = ModelFile {
    file_name: "gemma-4-E4B-it-Q4_0.gguf",
    url: "https://huggingface.co/ggml-org/gemma-4-E4B-it-GGUF/resolve/a69f018d7678a2b9f5e705f8903d6fb29b9b25dc/gemma-4-E4B-it-Q4_0.gguf",
    size_bytes: 4_590_807_392,
    sha256: "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2",
};

const BGE_M3: ModelFile = ModelFile {
    file_name: "bge-m3-q8_0.gguf",
    url: "https://huggingface.co/ggml-org/bge-m3-Q8_0-GGUF/resolve/9eba04c5d75ba5a1595e45de734d36bef4e5cb98/bge-m3-q8_0.gguf",
    size_bytes: 634_553_760,
    sha256: "aa473d51f451a22f0fcf39ba3330c14bed38a385712b1113440f69df4047a173",
};

const NOMIC_EMBED_V2: ModelFile = ModelFile {
    file_name: "nomic-embed-text-v2-moe-q8_0.gguf",
    url: "https://huggingface.co/ggml-org/Nomic-Embed-Text-V2-GGUF/resolve/498da4a128ed12a423efb6f9b0242dbac80209bf/nomic-embed-text-v2-moe-q8_0.gguf",
    size_bytes: 512_225_120,
    sha256: "36c5817bc25f379e62021f49efde05b10ed3b0c93ab8059c43173a7a5de73565",
};

const QWEN3_VL_2B: ModelFile = ModelFile {
    file_name: "Qwen3-VL-2B-Instruct-Q8_0.gguf",
    url: "https://huggingface.co/ggml-org/Qwen3-VL-2B-Instruct-GGUF/resolve/ea6a11058182570be6436b9a2e4ee7f7b49f908d/Qwen3-VL-2B-Instruct-Q8_0.gguf",
    size_bytes: 1_834_427_296,
    sha256: "b7802e29f71a9e5b5e3f83f613df898a2204342dcea71a231ea501d481813c39",
};

const QWEN3_VL_2B_PROJECTOR: ModelFile = ModelFile {
    file_name: "mmproj-Qwen3-VL-2B-Instruct-Q8_0.gguf",
    url: "https://huggingface.co/ggml-org/Qwen3-VL-2B-Instruct-GGUF/resolve/ea6a11058182570be6436b9a2e4ee7f7b49f908d/mmproj-Qwen3-VL-2B-Instruct-Q8_0.gguf",
    size_bytes: 445_053_056,
    sha256: "69066c8f279ec85ff48ab4059f6ebba0d2932ca57667f2bbdac7d9805bca9e7b",
};

/// Modelli proposti, nell'ordine in cui vengono mostrati.
pub const CATALOG: &[CatalogModel] = &[
    CatalogModel {
        id: "gemma-4-e2b",
        role: ModelRole::Chat,
        name: "Gemma 4 E2B",
        license: "Apache 2.0",
        memory_gb: 4.0,
        recommended: true,
        model: GEMMA4_E2B,
        projector: None,
    },
    CatalogModel {
        id: "qwen3.5-0.8b",
        role: ModelRole::Chat,
        name: "Qwen3.5 0.8B",
        license: "Apache 2.0",
        memory_gb: 1.5,
        recommended: false,
        model: QWEN35_08B,
        projector: None,
    },
    CatalogModel {
        id: "gemma-4-e4b",
        role: ModelRole::Chat,
        name: "Gemma 4 E4B",
        license: "Apache 2.0",
        memory_gb: 6.0,
        recommended: false,
        model: GEMMA4_E4B,
        projector: None,
    },
    CatalogModel {
        id: "bge-m3",
        role: ModelRole::Embedding,
        name: "BGE-M3",
        license: "MIT",
        memory_gb: 1.0,
        recommended: true,
        model: BGE_M3,
        projector: None,
    },
    CatalogModel {
        id: "nomic-embed-text-v2",
        role: ModelRole::Embedding,
        name: "Nomic Embed Text v2",
        license: "Apache 2.0",
        memory_gb: 1.0,
        recommended: false,
        model: NOMIC_EMBED_V2,
        projector: None,
    },
    CatalogModel {
        id: "qwen3-vl-2b",
        role: ModelRole::Vision,
        name: "Qwen3-VL 2B",
        license: "Apache 2.0",
        memory_gb: 3.5,
        recommended: true,
        model: QWEN3_VL_2B,
        projector: Some(QWEN3_VL_2B_PROJECTOR),
    },
    CatalogModel {
        id: "gemma-4-e2b-vision",
        role: ModelRole::Vision,
        name: "Gemma 4 E2B",
        license: "Apache 2.0",
        memory_gb: 4.5,
        recommended: false,
        // Stesso file della chat: se è già scaricato serve solo il proiettore.
        model: GEMMA4_E2B,
        projector: Some(GEMMA4_E2B_PROJECTOR),
    },
];

pub fn find(id: &str) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|model| model.id == id)
}

/// Modello del catalogo con il ruolo richiesto.
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
        for model in CATALOG {
            assert!(!ids.contains(&model.id), "id duplicato {}", model.id);
            ids.push(model.id);
            assert_eq!(model.projector.is_some(), model.role == ModelRole::Vision);
            for file in model.files() {
                assert_eq!(file.sha256.len(), 64);
                assert!(file.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                assert!(file.url.starts_with("https://huggingface.co/"));
                // Revisione fissa (hash del commit), mai un ramo mobile.
                assert!(!file.url.contains("/resolve/main/"), "{}", file.url);
                assert!(file.url.ends_with(file.file_name));
                assert!(file.file_name.ends_with(".gguf"));
                assert!(
                    file.file_name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                );
                // Lo stesso nome locale indica sempre lo stesso file.
                if let Some(previous) = files.insert(file.file_name, file) {
                    assert_eq!(previous, file);
                }
            }
        }
        for role in [ModelRole::Chat, ModelRole::Embedding, ModelRole::Vision] {
            let recommended = CATALOG
                .iter()
                .filter(|model| model.role == role && model.recommended)
                .count();
            assert_eq!(recommended, 1, "{role:?}");
        }
        assert!(find_with_role("bge-m3", ModelRole::Embedding).is_some());
        assert!(find_with_role("bge-m3", ModelRole::Chat).is_none());
        assert!(find("sconosciuto").is_none());
    }
}
