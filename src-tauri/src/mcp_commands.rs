//! Server MCP locale: modalità `greatnote --mcp` (stdin/stdout, senza
//! finestra) e comandi delle impostazioni per attivarlo e vederne il
//! registro. Il protocollo e gli strumenti sono in `greatnote_core::mcp`.

use std::{
    error::Error,
    io::{self, BufReader},
    path::{Path, PathBuf},
};

use greatnote_core::{
    data_dir::DATABASE_FILE,
    mcp::{AuditEntry, McpServer, McpSettings, read_audit},
    storage::Database,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

type CommandResult<T> = Result<T, String>;

/// Identificatore dell'app (`tauri.conf.json`), che dà il nome alla
/// cartella dati quando il server parte senza Tauri.
const APP_IDENTIFIER: &str = "dev.greatnote.notes";
const LEGACY_IDENTIFIER: &str = "dev.gna.notes";
pub const MCP_FLAG: &str = "--mcp";
const DATA_DIR_FLAG: &str = "--data-dir";

/// Cartella dati: quella indicata con `--data-dir` oppure la stessa
/// dell'app (con la cartella del vecchio nome se è l'unica con le note).
fn data_directory(args: &[String]) -> Result<PathBuf, Box<dyn Error>> {
    if let Some(index) = args.iter().position(|arg| arg == DATA_DIR_FLAG) {
        let path = args
            .get(index + 1)
            .ok_or("--data-dir richiede un percorso")?;
        return Ok(PathBuf::from(path));
    }
    let base = dirs::data_dir().ok_or("cartella dati dell'utente non trovata")?;
    let current = base.join(APP_IDENTIFIER);
    let legacy = base.join(LEGACY_IDENTIFIER);
    if !current.join(DATABASE_FILE).exists() && legacy.join(DATABASE_FILE).exists() {
        return Ok(legacy);
    }
    Ok(current)
}

/// Avvia il server MCP sullo standard input/output. I log vanno su stderr:
/// stdout è riservato ai messaggi del protocollo.
pub fn run_mcp(args: &[String]) -> Result<(), Box<dyn Error>> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .init();
    let directory = data_directory(args)?;
    let path = directory.join(DATABASE_FILE);
    if !path.exists() {
        return Err(format!(
            "database non trovato in {}: apri Greatnote almeno una volta",
            directory.display()
        )
        .into());
    }
    let database = Database::open_read_only(&path)?;
    let server = McpServer::new(database, directory);
    server.serve(BufReader::new(io::stdin().lock()), io::stdout().lock())?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct McpStatus {
    pub enabled: bool,
    /// Strumenti di modifica consentiti.
    pub allow_write: bool,
    /// Programma da avviare nella configurazione del client MCP.
    pub command: String,
    pub args: Vec<String>,
    /// Ultime chiamate, dalla più recente.
    pub audit: Vec<AuditEntry>,
}

/// Eseguibile da indicare al client: in un AppImage il percorso stabile è
/// quello del file AppImage, non quello del montaggio temporaneo.
fn executable() -> String {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "greatnote".to_owned())
}

fn status(database: &Database) -> CommandResult<McpStatus> {
    let settings = database.mcp_settings().map_err(|error| error.to_string())?;
    let directory = database
        .data_directory()
        .map(Path::to_path_buf)
        .map_err(|error| error.to_string())?;
    Ok(McpStatus {
        enabled: settings.enabled,
        allow_write: settings.allow_write,
        command: executable(),
        args: vec![
            MCP_FLAG.to_owned(),
            DATA_DIR_FLAG.to_owned(),
            directory.display().to_string(),
        ],
        audit: read_audit(&directory, 20),
    })
}

#[tauri::command]
pub fn get_mcp_status(database: State<'_, Database>) -> CommandResult<McpStatus> {
    status(&database)
}

#[tauri::command]
pub fn set_mcp_enabled(app: AppHandle, enabled: bool) -> CommandResult<McpStatus> {
    let database = app.state::<Database>();
    let current = database.mcp_settings().map_err(|error| error.to_string())?;
    database
        .set_mcp_settings(&McpSettings { enabled, ..current })
        .map_err(|error| error.to_string())?;
    tracing::info!(enabled, "server MCP aggiornato");
    status(&database)
}

/// Consente o blocca gli strumenti di modifica (creare note, aggiungere
/// testo, creare e completare attività). Ha effetto alla chiamata
/// successiva del client.
#[tauri::command]
pub fn set_mcp_allow_write(app: AppHandle, allowed: bool) -> CommandResult<McpStatus> {
    let database = app.state::<Database>();
    let current = database.mcp_settings().map_err(|error| error.to_string())?;
    database
        .set_mcp_settings(&McpSettings {
            allow_write: allowed,
            ..current
        })
        .map_err(|error| error.to_string())?;
    tracing::info!(allowed, "modifiche del server MCP aggiornate");
    status(&database)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_matches_tauri_config() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("config");
        assert_eq!(config["identifier"], APP_IDENTIFIER);
    }

    /// Le immagini nelle note sono mostrate come `blob:`: senza questa
    /// voce la build di produzione (dove la CSP è applicata) non le mostra.
    #[test]
    fn content_security_policy_allows_note_images_and_nothing_remote() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("config");
        for key in ["csp", "devCsp"] {
            let csp = config["app"]["security"][key].as_str().expect("csp");
            let images = csp
                .split(';')
                .map(str::trim)
                .find(|directive| directive.starts_with("img-src"))
                .expect("img-src");
            assert!(
                images.split_whitespace().any(|source| source == "blob:"),
                "{key}: {images}"
            );
            assert!(
                !csp.contains("https:") && !csp.contains('*'),
                "{key} consente origini remote"
            );
        }
    }

    /// Discover e GNOME Software mostrano la versione dai metadati AppStream
    /// dei pacchetti Linux: ogni rilascio deve comparire in `<releases>`.
    #[test]
    fn appstream_metadata_lists_the_current_version() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("config");
        let version = config["version"].as_str().expect("version");
        let metainfo = include_str!("../linux/dev.greatnote.notes.metainfo.xml");
        assert!(metainfo.contains(&format!("<id>{APP_IDENTIFIER}</id>")));
        assert!(
            metainfo.contains(&format!("<release version=\"{version}\"")),
            "aggiungere la versione {version} in linux/dev.greatnote.notes.metainfo.xml"
        );
    }

    #[test]
    fn explicit_data_directory() {
        let args = vec![
            "--mcp".to_owned(),
            "--data-dir".to_owned(),
            "/tmp/dati".to_owned(),
        ];
        assert_eq!(
            data_directory(&args).expect("dir"),
            PathBuf::from("/tmp/dati")
        );
        assert!(data_directory(&["--mcp".to_owned(), "--data-dir".to_owned()]).is_err());
    }
}
