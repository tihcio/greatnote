//! Ricerca dei modelli GGUF su Hugging Face per il provider integrato
//! (ADR 0021).
//!
//! - La ricerca usa l'API pubblica `/api/models`, senza account né token: i
//!   repository riservati (`gated`) o privati non sono scaricabili.
//! - I dettagli di un repository sono letti a una revisione precisa (hash del
//!   commit): URL, dimensione e SHA-256 di ogni file vengono da lì e il
//!   download li verifica ([`super::store`]).
//! - Le risposte sono dati non attendibili: nomi di repository, percorsi e
//!   hash vengono controllati prima di entrare in un URL o in un nome di file.

use std::{collections::BTreeMap, time::Duration};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use ureq::Agent;

use super::{
    catalog::{
        CatalogModel, ModelFile, ModelRole, ModelSource, hub_model_id, is_safe_file_name,
        is_valid_repo_id, is_valid_revision,
    },
    hardware::{HardwareInfo, ModelFit, estimated_memory, model_fit},
};
use crate::ai::AiError;

const API: &str = "https://huggingface.co/api/models";
const SITE: &str = "https://huggingface.co";
pub const MAX_QUERY_CHARS: usize = 100;
const SEARCH_LIMIT: usize = 30;
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SPLIT_PARTS: usize = 64;
/// Modelli più grandi non sono adatti a nessun computer da ufficio.
const MAX_VARIANT_BYTES: u64 = 256 * 1024 * 1024 * 1024;
const MAX_VARIANTS: usize = 80;
const EMBEDDING_PIPELINES: [&str; 2] = ["feature-extraction", "sentence-similarity"];
const VISION_PIPELINES: [&str; 2] = ["image-text-to-text", "any-to-any"];

/// Risultato della ricerca.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HubRepo {
    pub id: String,
    pub downloads: u64,
    pub likes: u64,
    pub license: Option<String>,
    pub pipeline_tag: Option<String>,
    pub architecture: Option<String>,
    /// Richiede di accettare condizioni o un accesso: non scaricabile.
    pub gated: bool,
    pub last_modified: Option<String>,
}

/// Una variante scaricabile (di solito una quantizzazione).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HubVariant {
    pub model: CatalogModel,
    pub quantization: Option<String>,
    pub fit: ModelFit,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HubRepoDetails {
    pub repo: String,
    pub revision: String,
    pub role: ModelRole,
    pub license: Option<String>,
    pub pipeline_tag: Option<String>,
    pub architecture: Option<String>,
    pub variants: Vec<HubVariant>,
    /// Indice della variante consigliata per questo computer.
    pub recommended: Option<usize>,
    /// Per la visione: il repository contiene un proiettore (`mmproj`).
    pub has_projector: bool,
}

/// Client dell'API pubblica: solo https, risposte di dimensione limitata.
pub struct HubClient {
    agent: Agent,
}

impl Default for HubClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HubClient {
    pub fn new() -> Self {
        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .max_redirects(3)
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(concat!("Greatnote/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self { agent }
    }

    /// Repository GGUF più scaricati che corrispondono a `query` (anche
    /// vuota), adatti al ruolo.
    pub fn search(&self, query: &str, role: ModelRole) -> Result<Vec<HubRepo>, AiError> {
        let query = validate_query(query)?;
        let pipelines: &[&str] = match role {
            ModelRole::Chat => &[],
            ModelRole::Embedding => &EMBEDDING_PIPELINES,
            ModelRole::Vision => &VISION_PIPELINES,
        };
        let mut results = Vec::new();
        if pipelines.is_empty() {
            results.extend(self.search_once(&query, None)?);
        } else {
            for pipeline in pipelines {
                results.extend(self.search_once(&query, Some(pipeline))?);
            }
        }
        Ok(merge_results(results, role))
    }

    fn search_once(&self, query: &str, pipeline: Option<&str>) -> Result<Vec<HubRepo>, AiError> {
        let mut request = self
            .agent
            .get(API)
            .query("filter", "gguf")
            .query("sort", "downloads")
            .query("direction", "-1")
            .query("limit", SEARCH_LIMIT.to_string());
        if !query.is_empty() {
            request = request.query("search", query);
        }
        if let Some(pipeline) = pipeline {
            request = request.query("pipeline_tag", pipeline);
        }
        for field in [
            "downloads",
            "likes",
            "tags",
            "gated",
            "pipeline_tag",
            "lastModified",
            "gguf",
        ] {
            request = request.query("expand[]", field);
        }
        let body = read_response(request.call())?;
        parse_search(&body)
    }

    /// Varianti di un repository, alla revisione corrente fissata con il suo
    /// hash, con l'indicazione di quanto sono adatte a `hardware`.
    pub fn details(
        &self,
        repo: &str,
        role: ModelRole,
        hardware: &HardwareInfo,
    ) -> Result<HubRepoDetails, AiError> {
        if !is_valid_repo_id(repo) {
            return Err(AiError::Hub("nome del repository non valido".to_owned()));
        }
        let info = read_response(self.agent.get(&format!("{API}/{repo}")).call())?;
        let info = parse_repo_info(&info)?;
        let tree = read_response(
            self.agent
                .get(&format!("{API}/{repo}/tree/{}", info.revision))
                .query("recursive", "true")
                .call(),
        )?;
        let files = parse_tree(&tree)?;
        Ok(build_details(repo, role, info, &files, hardware))
    }
}

fn validate_query(query: &str) -> Result<String, AiError> {
    let query = query.trim();
    if query.chars().count() > MAX_QUERY_CHARS || query.chars().any(char::is_control) {
        return Err(AiError::Hub(format!(
            "ricerca non valida: al massimo {MAX_QUERY_CHARS} caratteri"
        )));
    }
    Ok(query.to_owned())
}

fn read_response(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<String, AiError> {
    let mut response = result.map_err(|error| match error {
        ureq::Error::Timeout(_) => {
            AiError::Hub("Hugging Face non risponde, riprova più tardi".to_owned())
        }
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
            AiError::Hub("connessione non riuscita: controlla la rete".to_owned())
        }
        other => AiError::Hub(other.to_string()),
    })?;
    match response.status().as_u16() {
        200 => {}
        // Hugging Face risponde 401 anche per i repository inesistenti.
        401 | 403 | 404 => {
            return Err(AiError::Hub(
                "repository non trovato oppure privato".to_owned(),
            ));
        }
        429 => {
            return Err(AiError::Hub(
                "troppe richieste a Hugging Face: riprova tra qualche minuto".to_owned(),
            ));
        }
        status => {
            return Err(AiError::Hub(format!(
                "Hugging Face ha risposto con il codice {status}"
            )));
        }
    }
    response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|error| AiError::Hub(format!("risposta non letta: {error}")))
}

fn invalid(what: &str) -> AiError {
    AiError::Hub(format!("risposta non valida ({what})"))
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| text.chars().filter(|c| !c.is_control()).take(200).collect())
}

/// Licenza dai tag (`license:apache-2.0`).
fn license_from_tags(value: &Value) -> Option<String> {
    value
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .find_map(|tag| tag.strip_prefix("license:"))
        .filter(|license| !license.is_empty() && license.len() <= 60)
        .map(str::to_owned)
}

/// `false`, oppure `"auto"`/`"manual"` per i repository riservati.
fn is_gated(value: &Value) -> bool {
    match value.get("gated") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(_) => true,
    }
}

fn architecture(value: &Value) -> Option<String> {
    string_field(value.get("gguf")?, "architecture").filter(|name| name.len() <= 60)
}

pub fn parse_search(body: &str) -> Result<Vec<HubRepo>, AiError> {
    let value: Value = serde_json::from_str(body).map_err(|_| invalid("ricerca"))?;
    let entries = value.as_array().ok_or_else(|| invalid("ricerca"))?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?;
            if !is_valid_repo_id(id) || entry.get("private").and_then(Value::as_bool) == Some(true)
            {
                return None;
            }
            Some(HubRepo {
                id: id.to_owned(),
                downloads: entry.get("downloads").and_then(Value::as_u64).unwrap_or(0),
                likes: entry.get("likes").and_then(Value::as_u64).unwrap_or(0),
                license: license_from_tags(entry),
                pipeline_tag: string_field(entry, "pipeline_tag"),
                architecture: architecture(entry),
                gated: is_gated(entry),
                last_modified: string_field(entry, "lastModified"),
            })
        })
        .collect())
}

/// Unisce i risultati di più ricerche, senza doppioni, in ordine di
/// download. Per la chat esclude i modelli di solo embedding.
fn merge_results(results: Vec<HubRepo>, role: ModelRole) -> Vec<HubRepo> {
    let mut unique: BTreeMap<String, HubRepo> = BTreeMap::new();
    for repo in results {
        let embedding_only = repo
            .pipeline_tag
            .as_deref()
            .is_some_and(|tag| EMBEDDING_PIPELINES.contains(&tag));
        if role == ModelRole::Chat && embedding_only {
            continue;
        }
        unique.entry(repo.id.clone()).or_insert(repo);
    }
    let mut merged: Vec<HubRepo> = unique.into_values().collect();
    merged.sort_by(|a, b| b.downloads.cmp(&a.downloads).then(a.id.cmp(&b.id)));
    merged.truncate(SEARCH_LIMIT);
    merged
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfo {
    pub revision: String,
    pub license: Option<String>,
    pub pipeline_tag: Option<String>,
    pub architecture: Option<String>,
}

pub fn parse_repo_info(body: &str) -> Result<RepoInfo, AiError> {
    let value: Value = serde_json::from_str(body).map_err(|_| invalid("repository"))?;
    if is_gated(&value) {
        return Err(AiError::Hub(
            "il repository richiede di accettare le condizioni d'uso su Hugging Face: non è scaricabile dall'app"
                .to_owned(),
        ));
    }
    if value.get("disabled").and_then(Value::as_bool) == Some(true)
        || value.get("private").and_then(Value::as_bool) == Some(true)
    {
        return Err(AiError::Hub("repository non disponibile".to_owned()));
    }
    let revision = value
        .get("sha")
        .and_then(Value::as_str)
        .filter(|sha| is_valid_revision(sha))
        .ok_or_else(|| invalid("revisione"))?;
    Ok(RepoInfo {
        revision: revision.to_owned(),
        license: license_from_tags(&value),
        pipeline_tag: string_field(&value, "pipeline_tag"),
        architecture: architecture(&value),
    })
}

/// File GGUF del repository con dimensione e SHA-256 (dai metadati LFS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Percorso nel repository usabile così com'è in un URL e in un nome di
/// file: caratteri sicuri, al massimo due cartelle, nessun `..`.
fn is_safe_repo_path(path: &str) -> bool {
    path.len() <= 200
        && !path.starts_with('/')
        && !path.contains("..")
        && !path.contains("//")
        && path.split('/').count() <= 3
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/'))
}

pub fn parse_tree(body: &str) -> Result<Vec<TreeFile>, AiError> {
    let value: Value = serde_json::from_str(body).map_err(|_| invalid("elenco dei file"))?;
    let entries = value.as_array().ok_or_else(|| invalid("elenco dei file"))?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            if entry.get("type")?.as_str()? != "file" {
                return None;
            }
            let path = entry.get("path")?.as_str()?;
            if !path.ends_with(".gguf") || !is_safe_repo_path(path) {
                return None;
            }
            let lfs = entry.get("lfs")?;
            let sha256 = lfs.get("oid")?.as_str()?;
            let size_bytes = lfs.get("size")?.as_u64()?;
            let sha_ok = sha256.len() == 64
                && sha256
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'));
            (sha_ok && size_bytes > 0).then(|| TreeFile {
                path: path.to_owned(),
                size_bytes,
                sha256: sha256.to_owned(),
            })
        })
        .collect())
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `nome-00002-of-00003.gguf` → (`nome`, 2, 3).
fn split_part(path: &str) -> Option<(&str, usize, usize)> {
    let stem = path.strip_suffix(".gguf")?;
    let (rest, count) = stem.rsplit_once("-of-")?;
    let (prefix, index) = rest.rsplit_once('-')?;
    let digits = |text: &str| text.len() == 5 && text.chars().all(|c| c.is_ascii_digit());
    if !digits(index) || !digits(count) {
        return None;
    }
    let (index, count) = (index.parse().ok()?, count.parse().ok()?);
    (index >= 1 && index <= count && count > 1).then_some((prefix, index, count))
}

fn is_projector(path: &str) -> bool {
    base_name(path).to_ascii_lowercase().starts_with("mmproj")
}

/// Teste per la generazione speculativa (MTP), matrici di importanza e simili:
/// non sono modelli da usare da soli.
fn is_auxiliary(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = base_name(&lower);
    name.starts_with("mtp-")
        || lower.starts_with("mtp/")
        || lower.contains("/mtp/")
        || name.contains("imatrix")
        || name.starts_with("draft-")
}

/// Quantizzazione dal nome del file (`...-Q4_K_M`, `.Q8_0`, `-UD-IQ2_M`...).
pub fn quantization(stem: &str) -> Option<String> {
    let upper = stem.to_ascii_uppercase();
    upper
        .rsplit(['-', '.'])
        .find(|token| {
            let quant =
                token.starts_with('Q') || token.starts_with("IQ") || token.starts_with("TQ");
            let digit_after = token
                .trim_start_matches(['I', 'T', 'Q'])
                .starts_with(|c: char| c.is_ascii_digit());
            (quant && digit_after) || matches!(*token, "F16" | "BF16" | "F32" | "MXFP4")
        })
        .map(str::to_owned)
}

fn is_unquantized(quantization: Option<&str>) -> bool {
    matches!(quantization, Some("F16" | "BF16" | "F32"))
}

/// Nome locale: prefisso legato a repository e revisione (lo stesso nome
/// indica sempre lo stesso contenuto) e percorso del file appiattito. Le parti
/// di un modello diviso mantengono lo schema `-0000N-of-0000M.gguf` che
/// llama.cpp usa per trovare le successive.
fn local_file_name(repo: &str, revision: &str, path: &str) -> String {
    let key = Sha256::digest(format!("{repo}@{revision}").as_bytes());
    let key: String = key
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("hf-{key}-{}", path.replace('/', "_"))
}

fn model_file(repo: &str, revision: &str, file: &TreeFile) -> ModelFile {
    ModelFile {
        file_name: local_file_name(repo, revision, &file.path),
        url: format!("{SITE}/{repo}/resolve/{revision}/{}", file.path),
        size_bytes: file.size_bytes,
        sha256: file.sha256.clone(),
    }
}

/// Proiettore preferito: Q8_0, poi F16, BF16, F32, poi il più piccolo.
fn pick_projector(files: &[TreeFile]) -> Option<&TreeFile> {
    let projectors: Vec<&TreeFile> = files
        .iter()
        .filter(|file| is_projector(&file.path) && split_part(&file.path).is_none())
        .collect();
    for preferred in ["Q8_0", "F16", "BF16", "F32"] {
        if let Some(file) = projectors.iter().find(|file| {
            quantization(file.path.trim_end_matches(".gguf")).as_deref() == Some(preferred)
        }) {
            return Some(file);
        }
    }
    projectors.into_iter().min_by_key(|file| file.size_bytes)
}

fn display_repo_name(repo: &str) -> String {
    let name = repo.rsplit('/').next().unwrap_or(repo);
    let lower = name.to_ascii_lowercase();
    let trimmed = if lower.ends_with("-gguf") || lower.ends_with("_gguf") {
        &name[..name.len() - 5]
    } else {
        name
    };
    trimmed.to_owned()
}

pub fn build_details(
    repo: &str,
    role: ModelRole,
    info: RepoInfo,
    files: &[TreeFile],
    hardware: &HardwareInfo,
) -> HubRepoDetails {
    let revision = info.revision.clone();
    let projector = pick_projector(files).map(|file| model_file(repo, &revision, file));
    let has_projector = projector.is_some();

    // Raggruppa le parti dei modelli divisi sotto lo stesso nome.
    let mut groups: BTreeMap<String, Vec<(usize, usize, &TreeFile)>> = BTreeMap::new();
    for file in files {
        if is_projector(&file.path) || is_auxiliary(&file.path) {
            continue;
        }
        match split_part(&file.path) {
            Some((prefix, index, count)) => groups
                .entry(prefix.to_owned())
                .or_default()
                .push((index, count, file)),
            None => groups
                .entry(file.path.trim_end_matches(".gguf").to_owned())
                .or_default()
                .push((1, 1, file)),
        }
    }

    let license = info
        .license
        .clone()
        .unwrap_or_else(|| "non indicata".to_owned());
    let mut variants = Vec::new();
    if role != ModelRole::Vision || has_projector {
        for (stem, mut parts) in groups {
            parts.sort_by_key(|(index, _, _)| *index);
            let count = parts[0].1;
            let complete = parts.len() == count
                && count <= MAX_SPLIT_PARTS
                && parts
                    .iter()
                    .enumerate()
                    .all(|(position, (index, total, _))| *index == position + 1 && *total == count);
            if !complete {
                continue;
            }
            let variant = stem.replace('/', "_");
            let quant = quantization(base_name(&stem));
            let mut part_files = parts
                .iter()
                .map(|(_, _, file)| model_file(repo, &revision, file));
            let Some(first) = part_files.next() else {
                continue;
            };
            let model = CatalogModel {
                id: hub_model_id(role, repo, &variant),
                role,
                name: format!(
                    "{} · {}",
                    display_repo_name(repo),
                    quant.clone().unwrap_or_else(|| base_name(&stem).to_owned())
                ),
                license: license.clone(),
                memory_gb: 0.0,
                recommended: false,
                model: first,
                parts: part_files.collect(),
                projector: if role == ModelRole::Vision {
                    projector.clone()
                } else {
                    None
                },
                source: ModelSource::HuggingFace {
                    repo: repo.to_owned(),
                    revision: revision.clone(),
                },
            };
            let size = model.size_bytes();
            if size > MAX_VARIANT_BYTES || !model.is_valid() {
                continue;
            }
            let memory = estimated_memory(size) as f64 / 1e9;
            let model = CatalogModel {
                memory_gb: ((memory * 10.0).round() / 10.0) as f32,
                ..model
            };
            variants.push(HubVariant {
                fit: model_fit(size, hardware),
                quantization: quant,
                model,
            });
        }
    }
    variants.sort_by_key(|variant| variant.model.size_bytes());
    variants.truncate(MAX_VARIANTS);
    let recommended = recommend(&variants);
    HubRepoDetails {
        repo: repo.to_owned(),
        revision,
        role,
        license: info.license,
        pipeline_tag: info.pipeline_tag,
        architecture: info.architecture,
        variants,
        recommended,
        has_projector,
    }
}

/// Variante consigliata: la più grande (quindi la più fedele) che sta nella
/// scheda video; altrimenti, in RAM, Q4_K_M o la più grande non oltre essa,
/// perché sul processore le varianti grandi sono troppo lente. Mai le
/// varianti non quantizzate.
pub fn recommend(variants: &[HubVariant]) -> Option<usize> {
    let candidates = || {
        variants
            .iter()
            .enumerate()
            .filter(|(_, variant)| !is_unquantized(variant.quantization.as_deref()))
    };
    if let Some((index, _)) = candidates()
        .filter(|(_, variant)| variant.fit == ModelFit::Gpu)
        .max_by_key(|(_, variant)| variant.model.size_bytes())
    {
        return Some(index);
    }
    let on_cpu = || candidates().filter(|(_, variant)| variant.fit == ModelFit::Cpu);
    let q4 = variants
        .iter()
        .find(|variant| variant.quantization.as_deref() == Some("Q4_K_M"))
        .map(|variant| variant.model.size_bytes());
    if let Some(limit) = q4
        && let Some((index, _)) = on_cpu()
            .filter(|(_, variant)| variant.model.size_bytes() <= limit)
            .max_by_key(|(_, variant)| variant.model.size_bytes())
    {
        return Some(index);
    }
    on_cpu()
        .min_by_key(|(_, variant)| variant.model.size_bytes())
        .map(|(index, _)| index)
}

/// Il nome locale rispetta le regole dei file dei modelli.
pub fn is_valid_local_name(name: &str) -> bool {
    is_safe_file_name(name) && name.starts_with("hf-")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;
    const REVISION: &str = "6938c58e698502037401a5f19f6905f2fb2f7dee";

    fn sha(seed: u8) -> String {
        format!("{seed:02x}").repeat(32)
    }

    fn tree_entry(path: &str, size: u64, seed: u8) -> String {
        format!(
            r#"{{"type":"file","oid":"x","size":{size},"path":"{path}","lfs":{{"oid":"{}","size":{size},"pointerSize":135}}}}"#,
            sha(seed)
        )
    }

    fn info() -> RepoInfo {
        RepoInfo {
            revision: REVISION.to_owned(),
            license: Some("apache-2.0".to_owned()),
            pipeline_tag: Some("image-text-to-text".to_owned()),
            architecture: Some("gemma4".to_owned()),
        }
    }

    #[test]
    fn search_results_are_parsed_merged_and_filtered_by_role() {
        // Estratto di una risposta reale di /api/models con expand[].
        let body = r#"[
          {"id":"unsloth/bge-small-en-v1.5-GGUF","gated":false,"likes":8,"downloads":217387,
           "tags":["gguf","feature-extraction","license:mit"],"pipeline_tag":"feature-extraction",
           "lastModified":"2026-06-09T11:00:55.000Z"},
          {"id":"meta-llama/Llama-Riservato-GGUF","gated":"manual","downloads":5,"tags":[]},
          {"id":"../fuga","downloads":999999},
          {"id":"unsloth/gemma-4-12b-it-GGUF","downloads":1568868,"likes":923,
           "tags":["gguf","license:apache-2.0"],"pipeline_tag":"image-text-to-text",
           "gguf":{"architecture":"gemma4","context_length":131072}}
        ]"#;
        let results = parse_search(body).expect("parsed");
        assert_eq!(results.len(), 3, "l'id non valido è scartato");
        assert_eq!(results[0].license.as_deref(), Some("mit"));
        assert!(results[1].gated);
        assert_eq!(results[2].architecture.as_deref(), Some("gemma4"));

        let chat = merge_results(results.clone(), ModelRole::Chat);
        assert_eq!(
            chat.iter().map(|repo| repo.id.as_str()).collect::<Vec<_>>(),
            [
                "unsloth/gemma-4-12b-it-GGUF",
                "meta-llama/Llama-Riservato-GGUF"
            ]
        );
        let doubled = [results.clone(), results].concat();
        let embedding = merge_results(doubled, ModelRole::Embedding);
        assert_eq!(embedding.len(), 3, "nessun doppione");
        assert!(parse_search("{}").is_err());
        assert!(validate_query(&"x".repeat(MAX_QUERY_CHARS + 1)).is_err());
        assert!(validate_query("gem\u{0}ma").is_err());
        assert_eq!(validate_query("  qwen  ").expect("query"), "qwen");
    }

    #[test]
    fn repo_info_requires_a_fixed_revision_and_rejects_gated_repos() {
        let info = parse_repo_info(&format!(
            r#"{{"sha":"{REVISION}","gated":false,"tags":["license:apache-2.0"],"pipeline_tag":"any-to-any","gguf":{{"architecture":"gemma4"}}}}"#
        ))
        .expect("info");
        assert_eq!(info.revision, REVISION);
        assert_eq!(info.license.as_deref(), Some("apache-2.0"));
        assert_eq!(info.architecture.as_deref(), Some("gemma4"));
        assert!(parse_repo_info(&format!(r#"{{"sha":"{REVISION}","gated":"auto"}}"#)).is_err());
        assert!(parse_repo_info(r#"{"sha":"main"}"#).is_err());
        assert!(parse_repo_info(&format!(r#"{{"sha":"{REVISION}","disabled":true}}"#)).is_err());
    }

    #[test]
    fn variants_group_split_parts_skip_auxiliary_files_and_pick_a_projector() {
        let body = format!(
            "[{}]",
            [
                r#"{"type":"directory","oid":"x","size":0,"path":"MTP"}"#.to_owned(),
                r#"{"type":"file","oid":"x","size":2098,"path":"README.md"}"#.to_owned(),
                tree_entry("MTP/mtp-model-Q8_0.gguf", GIB, 1),
                tree_entry("mtp-model.gguf", GIB, 2),
                tree_entry("model-Q4_K_M.gguf", 5 * GIB, 3),
                tree_entry("model-Q8_0.gguf", 9 * GIB, 4),
                tree_entry("model-BF16.gguf", 16 * GIB, 5),
                tree_entry("Q6_K/model-Q6_K-00001-of-00002.gguf", 4 * GIB, 6),
                tree_entry("Q6_K/model-Q6_K-00002-of-00002.gguf", 3 * GIB, 7),
                // Parte mancante: la variante divisa incompleta è scartata.
                tree_entry("model-Q3_K_S-00001-of-00002.gguf", 2 * GIB, 8),
                tree_entry("mmproj-F16.gguf", GIB / 2, 9),
                tree_entry("mmproj-Q8_0.gguf", GIB / 4, 10),
                tree_entry("../fuga.gguf", GIB, 11),
                tree_entry("nome con spazi.gguf", GIB, 12),
            ]
            .join(",")
        );
        let files = parse_tree(&body).expect("tree");
        assert_eq!(
            files.len(),
            10,
            "percorsi non sicuri e file non LFS scartati"
        );

        let hardware = HardwareInfo {
            ram_bytes: Some(32 * GIB),
            gpus: vec![super::super::hardware::GpuDevice {
                name: "NVIDIA".to_owned(),
                memory_bytes: 8 * GIB,
                dedicated: true,
            }],
        };
        let repo = "unsloth/model-GGUF";
        let chat = build_details(repo, ModelRole::Chat, info(), &files, &hardware);
        let names: Vec<_> = chat
            .variants
            .iter()
            .map(|variant| variant.quantization.clone().unwrap_or_default())
            .collect();
        assert_eq!(names, ["Q4_K_M", "Q6_K", "Q8_0", "BF16"]);
        let split = &chat.variants[1].model;
        assert_eq!(split.parts.len(), 1);
        assert_eq!(split.size_bytes(), 7 * GIB);
        assert!(
            split
                .model
                .file_name
                .ends_with("Q6_K_model-Q6_K-00001-of-00002.gguf")
        );
        assert!(
            split.parts[0]
                .file_name
                .ends_with("Q6_K_model-Q6_K-00002-of-00002.gguf")
        );
        assert_eq!(
            split.model.url,
            format!(
                "https://huggingface.co/{repo}/resolve/{REVISION}/Q6_K/model-Q6_K-00001-of-00002.gguf"
            )
        );
        assert_eq!(split.id, "hf:chat:unsloth/model-GGUF:Q6_K_model-Q6_K");
        assert!(
            chat.variants
                .iter()
                .all(|variant| variant.model.projector.is_none())
        );
        assert!(chat.variants.iter().all(|variant| variant.model.is_valid()));
        assert_eq!(chat.variants[0].model.name, "model · Q4_K_M");
        // 5 GB stanno nella scheda video da 8 GB, 7 GB no: il consigliato è
        // il più grande che ci sta.
        assert_eq!(chat.variants[0].fit, ModelFit::Gpu);
        assert_eq!(chat.recommended, Some(0));

        let vision = build_details(repo, ModelRole::Vision, info(), &files, &hardware);
        assert!(vision.has_projector);
        let projector = vision.variants[0]
            .model
            .projector
            .as_ref()
            .expect("projector");
        assert!(projector.file_name.ends_with("mmproj-Q8_0.gguf"));
        assert_eq!(vision.variants[0].model.size_bytes(), 5 * GIB + GIB / 4);

        let without_projector: Vec<TreeFile> = files
            .into_iter()
            .filter(|file| !is_projector(&file.path))
            .collect();
        let none = build_details(
            repo,
            ModelRole::Vision,
            info(),
            &without_projector,
            &hardware,
        );
        assert!(!none.has_projector);
        assert!(none.variants.is_empty());
    }

    #[test]
    fn local_names_are_stable_safe_and_keep_the_split_pattern() {
        let name = local_file_name("a/b", REVISION, "Q4/model-00001-of-00003.gguf");
        assert!(is_valid_local_name(&name), "{name}");
        assert!(name.ends_with("Q4_model-00001-of-00003.gguf"));
        assert_eq!(
            name,
            local_file_name("a/b", REVISION, "Q4/model-00001-of-00003.gguf")
        );
        assert_ne!(
            name,
            local_file_name(
                "a/b",
                "0000000000000000000000000000000000000000",
                "Q4/model-00001-of-00003.gguf"
            )
        );
        assert_eq!(split_part("x-00002-of-00003.gguf"), Some(("x", 2, 3)));
        assert_eq!(split_part("x-2-of-3.gguf"), None);
        assert_eq!(split_part("x-00004-of-00003.gguf"), None);
        assert_eq!(split_part("x.gguf"), None);
    }

    #[test]
    fn quantization_names_are_recognized() {
        assert_eq!(
            quantization("gemma-4-12b-it-Q4_K_M").as_deref(),
            Some("Q4_K_M")
        );
        assert_eq!(
            quantization("gemma-4-12b-it-UD-IQ2_M").as_deref(),
            Some("IQ2_M")
        );
        assert_eq!(quantization("bge-m3-q8_0").as_deref(), Some("Q8_0"));
        assert_eq!(quantization("model.Q5_K_S").as_deref(), Some("Q5_K_S"));
        assert_eq!(quantization("model-BF16").as_deref(), Some("BF16"));
        assert_eq!(quantization("Qwen3-0.6B"), None);
        assert_eq!(quantization("modello"), None);
    }

    #[test]
    fn recommendation_on_cpu_stays_at_q4_and_never_unquantized() {
        let variant = |quant: &str, size: u64, fit: ModelFit| {
            let files = [TreeFile {
                path: format!("m-{quant}.gguf"),
                size_bytes: size,
                sha256: sha(1),
            }];
            let details = build_details(
                "a/m",
                ModelRole::Chat,
                info(),
                &files,
                &HardwareInfo::default(),
            );
            HubVariant {
                fit,
                ..details.variants[0].clone()
            }
        };
        let variants = vec![
            variant("Q3_K_M", 3 * GIB, ModelFit::Cpu),
            variant("Q4_K_M", 4 * GIB, ModelFit::Cpu),
            variant("Q8_0", 8 * GIB, ModelFit::Cpu),
            variant("F16", 15 * GIB, ModelFit::Cpu),
        ];
        assert_eq!(recommend(&variants), Some(1));
        let only_large = vec![
            variant("Q8_0", 8 * GIB, ModelFit::Cpu),
            variant("BF16", 15 * GIB, ModelFit::Gpu),
        ];
        assert_eq!(recommend(&only_large), Some(0));
        let too_large = vec![variant("Q8_0", 8 * GIB, ModelFit::TooLarge)];
        assert_eq!(recommend(&too_large), None);
    }

    /// Prova con il vero Hugging Face (rete necessaria):
    /// `cargo test -p greatnote-core hub_live -- --ignored`
    #[test]
    #[ignore = "richiede la rete"]
    fn hub_live_search_and_details_match_the_curated_catalog() {
        let client = HubClient::new();
        let results = client
            .search("bge-m3", ModelRole::Embedding)
            .expect("search");
        assert!(!results.is_empty());
        let hardware = HardwareInfo {
            ram_bytes: Some(16 * GIB),
            gpus: Vec::new(),
        };
        // Repository usato dal catalogo curato: gli hash devono coincidere.
        let details = client
            .details("ggml-org/gemma-4-E2B-it-GGUF", ModelRole::Vision, &hardware)
            .expect("details");
        assert!(details.has_projector);
        let q4 = details
            .variants
            .iter()
            .find(|variant| variant.quantization.as_deref() == Some("Q4_0"))
            .expect("Q4_0");
        assert_eq!(
            q4.model.model.sha256,
            "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52"
        );
        assert!(details.recommended.is_some());
        assert!(client.details("../x", ModelRole::Chat, &hardware).is_err());
        assert!(
            client
                .details("nonexistent-zz/nope-zz", ModelRole::Chat, &hardware)
                .is_err()
        );
    }
}
