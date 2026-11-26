//! Server MCP locale: modalità `greatnote --mcp` (stdin/stdout, senza
//! finestra) e comandi delle impostazioni per attivarlo e vederne il
//! registro. Il protocollo e gli strumenti sono in `greatnote_core::mcp`.

use std::path::{Path, PathBuf};

use greatnote_core::{
    mcp::{AuditEntry, McpSettings, read_audit},
    mcp_requests::McpRequest,
    storage::Database,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

type CommandResult<T> = Result<T, String>;

pub const MCP_FLAG: &str = "--mcp";
const DATA_DIR_FLAG: &str = "--data-dir";

#[cfg(desktop)]
pub use stdio::run_mcp;

/// Modalità `greatnote --mcp`: esiste solo dove l'app è un eseguibile
/// avviabile da un client MCP, quindi non su Android e iOS.
#[cfg(desktop)]
mod stdio {
    use std::{
        error::Error,
        io::{self, BufReader},
        path::PathBuf,
    };

    use greatnote_core::{data_dir::DATABASE_FILE, mcp::McpServer, storage::Database};

    use super::DATA_DIR_FLAG;

    /// Identificatore dell'app (`tauri.conf.json`), che dà il nome alla
    /// cartella dati quando il server parte senza Tauri.
    pub(super) const APP_IDENTIFIER: &str = "dev.greatnote.notes";
    const LEGACY_IDENTIFIER: &str = "dev.gna.notes";

    /// Cartella dati: quella indicata con `--data-dir` oppure la stessa
    /// dell'app (con la cartella del vecchio nome se è l'unica con le note).
    pub(super) fn data_directory(args: &[String]) -> Result<PathBuf, Box<dyn Error>> {
        if let Some(index) = args.iter().position(|arg| arg == DATA_DIR_FLAG) {
            let path = args.get(index + 1).ok_or_else(|| {
                greatnote_core::tr!(
                    "--data-dir richiede un percorso",
                    "--data-dir requires a path"
                )
            })?;
            return Ok(PathBuf::from(path));
        }
        let base = dirs::data_dir().ok_or_else(|| {
            greatnote_core::tr!(
                "cartella dati dell'utente non trovata",
                "user data folder not found"
            )
        })?;
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
            return Err(greatnote_core::tr!(
                "database non trovato in {}: apri Greatnote almeno una volta",
                "database not found in {}: open Greatnote at least once",
                directory.display()
            )
            .into());
        }
        let database = Database::open_read_only(&path)?;
        let server = McpServer::new(database, directory);
        server.serve(BufReader::new(io::stdin().lock()), io::stdout().lock())?;
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct McpStatus {
    pub enabled: bool,
    /// Strumenti di modifica consentiti.
    pub allow_write: bool,
    /// Richieste di cestino e riscrittura (da confermare nell'app).
    pub allow_destructive: bool,
    /// Programma da avviare nella configurazione del client MCP.
    pub command: String,
    pub args: Vec<String>,
    /// Ultime chiamate, dalla più recente.
    pub audit: Vec<AuditEntry>,
}

/// Eseguibile da indicare al client: in un AppImage il percorso stabile è
/// quello del file AppImage, non quello del montaggio temporaneo; nel
/// pacchetto MSIX è l'alias di esecuzione dichiarato nel manifest, perché
/// gli altri programmi non possono avviare i file in `WindowsApps`.
fn executable() -> String {
    #[cfg(windows)]
    if crate::msix::is_packaged() {
        return dirs::data_local_dir()
            .map(|local| {
                local
                    .join("Microsoft")
                    .join("WindowsApps")
                    .join("greatnote.exe")
            })
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "greatnote.exe".to_owned());
    }
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
        allow_destructive: settings.allow_destructive,
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

/// Consente o blocca le richieste distruttive (cestino, riscrittura).
/// Bloccarle non cancella quelle già in attesa: restano da decidere.
#[tauri::command]
pub fn set_mcp_allow_destructive(app: AppHandle, allowed: bool) -> CommandResult<McpStatus> {
    let database = app.state::<Database>();
    let current = database.mcp_settings().map_err(|error| error.to_string())?;
    database
        .set_mcp_settings(&McpSettings {
            allow_destructive: allowed,
            ..current
        })
        .map_err(|error| error.to_string())?;
    tracing::info!(allowed, "richieste distruttive del server MCP aggiornate");
    status(&database)
}

/// Richieste del client MCP in attesa di conferma.
#[tauri::command]
pub fn list_mcp_requests(database: State<'_, Database>) -> CommandResult<Vec<McpRequest>> {
    database
        .pending_mcp_requests()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn approve_mcp_request(app: AppHandle, id: String) -> CommandResult<McpRequest> {
    let request = app
        .state::<Database>()
        .approve_mcp_request(&id)
        .map_err(|error| error.to_string())?;
    tracing::info!(action = ?request.action, "richiesta MCP confermata");
    Ok(request)
}

#[tauri::command]
pub fn reject_mcp_request(app: AppHandle, id: String) -> CommandResult<McpRequest> {
    let request = app
        .state::<Database>()
        .reject_mcp_request(&id)
        .map_err(|error| error.to_string())?;
    tracing::info!(action = ?request.action, "richiesta MCP rifiutata");
    Ok(request)
}

/// Evento per la UI: le richieste in attesa sono cambiate.
#[cfg(desktop)]
pub const MCP_REQUESTS_EVENT: &str = "mcp-requests-changed";

/// Controlla ogni pochi secondi le richieste accodate dal client MCP (che
/// gira in un altro processo): per le nuove avvisa la UI e manda una
/// notifica di sistema, perché l'utente di solito sta guardando il client.
#[cfg(desktop)]
pub fn spawn_request_watcher(app: &AppHandle) {
    use std::{collections::HashSet, thread, time::Duration};

    use tauri::Emitter;

    const INTERVAL: Duration = Duration::from_secs(3);
    let app = app.clone();
    let spawned = thread::Builder::new()
        .name("mcp-requests".to_owned())
        .spawn(move || {
            let mut known: HashSet<String> = HashSet::new();
            loop {
                thread::sleep(INTERVAL);
                let database = app.state::<Database>();
                let allowed = database
                    .mcp_settings()
                    .is_ok_and(|settings| settings.destructive_allowed());
                // Con il permesso tolto le richieste già arrivate restano
                // visibili nell'app, ma non se ne controllano di nuove.
                if !allowed && known.is_empty() {
                    continue;
                }
                let pending = match database.pending_mcp_requests() {
                    Ok(pending) => pending,
                    Err(error) => {
                        tracing::warn!("richieste MCP non lette: {error}");
                        continue;
                    }
                };
                let ids: HashSet<String> =
                    pending.iter().map(|request| request.id.clone()).collect();
                if ids == known {
                    continue;
                }
                let fresh: Vec<&McpRequest> = pending
                    .iter()
                    .filter(|request| !known.contains(&request.id))
                    .collect();
                known = ids;
                if let Err(error) = app.emit(MCP_REQUESTS_EVENT, pending.len()) {
                    tracing::warn!("evento richieste MCP non inviato: {error}");
                }
                if let Some(first) = fresh.first() {
                    let english = app
                        .state::<crate::planner_commands::UiLanguage>()
                        .is_english();
                    let (title, body) = notification_text(first, fresh.len(), english);
                    if let Err(error) =
                        crate::planner_commands::send_notification(&app, &title, &body)
                    {
                        tracing::warn!("notifica richiesta MCP non inviata: {error}");
                    }
                }
            }
        });
    if let Err(error) = spawned {
        tracing::error!("controllo delle richieste MCP non avviato: {error}");
    }
}

/// Testo della notifica: azione e titolo della nota, mai il testo proposto.
#[cfg(desktop)]
fn notification_text(request: &McpRequest, count: usize, english: bool) -> (String, String) {
    use greatnote_core::mcp_requests::McpAction;
    let note = if request.note_title.trim().is_empty() {
        if english { "Untitled" } else { "Senza titolo" }.to_owned()
    } else {
        request.note_title.clone()
    };
    let title = if english {
        "An AI app is asking for confirmation"
    } else {
        "Un'app di AI chiede una conferma"
    }
    .to_owned();
    let action = match (request.action, english) {
        (McpAction::TrashNote, false) => format!("Spostare «{note}» nel cestino"),
        (McpAction::TrashNote, true) => format!("Move “{note}” to the trash"),
        (McpAction::ReplaceNote, false) => format!("Riscrivere «{note}»"),
        (McpAction::ReplaceNote, true) => format!("Rewrite “{note}”"),
    };
    let body = match (count, english) {
        (1, false) => format!("{action}. Apri Greatnote per vedere l'anteprima."),
        (1, true) => format!("{action}. Open Greatnote to review it."),
        (_, false) => format!("{action} e altre {} richieste.", count - 1),
        (_, true) => format!("{action} and {} more requests.", count - 1),
    };
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::{
        stdio::{APP_IDENTIFIER, data_directory},
        *,
    };

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
