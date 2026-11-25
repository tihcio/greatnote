//! Note create dal pannello «Condividi» di Android (testo, link, immagini,
//! PDF e file di testo da altre app). Sul desktop non arriva mai niente.

use std::{fs, path::Path};

use greatnote_core::{
    share::{SharedFile, SharedNote},
    storage::{Database, MAX_ATTACHMENT_BYTES},
};
use tauri::{AppHandle, Manager};

use crate::{data_commands::blocking, file_access};

type CommandResult<T> = Result<T, String>;

/// Cartella in cui `FilesPlugin.takeShare` copia i file ricevuti.
const SHARED_DIRECTORY: &str = "shared";

/// Se un'altra app ha condiviso qualcosa con Greatnote, crea la nota e la
/// restituisce (con i file saltati); altrimenti `None`. Il frontend la
/// chiama all'avvio e a ogni ritorno in primo piano.
#[tauri::command]
pub async fn take_shared_content(app: AppHandle) -> CommandResult<Option<SharedNote>> {
    blocking(app, |app| {
        let Some(payload) = file_access::take_shared(app)? else {
            return Ok(None);
        };
        let directory = app
            .path()
            .app_cache_dir()
            .map_err(|error| error.to_string())?
            .join(SHARED_DIRECTORY);
        let files = read_shared_files(&directory, &payload.files);
        let database = app.state::<Database>();
        let data_directory = database
            .data_directory()
            .map_err(|error| error.to_string())?
            .to_path_buf();
        let result = database
            .create_note_from_share(
                payload.subject.as_deref(),
                payload.text.as_deref(),
                &files,
                &data_directory,
            )
            .map(Some)
            .map_err(|error| error.to_string());
        let _ = fs::remove_dir_all(&directory);
        result
    })
    .await
}

/// Legge le copie, accettando solo file dentro la cartella condivisa (i
/// percorsi arrivano dal plugin, ma vengono comunque controllati).
fn read_shared_files(directory: &Path, copies: &[file_access::SharedFileCopy]) -> Vec<SharedFile> {
    let Ok(root) = directory.canonicalize() else {
        return Vec::new();
    };
    copies
        .iter()
        .filter_map(|copy| {
            let path = copy.path.canonicalize().ok()?;
            if path.parent() != Some(root.as_path()) {
                tracing::warn!("file condiviso fuori dalla cartella prevista ignorato");
                return None;
            }
            // Un byte oltre il limite basta per riconoscerlo troppo grande.
            let size = fs::metadata(&path).ok()?.len() as usize;
            let bytes = if size > MAX_ATTACHMENT_BYTES {
                vec![0; MAX_ATTACHMENT_BYTES + 1]
            } else {
                fs::read(&path).ok()?
            };
            Some(SharedFile {
                name: copy.name.clone(),
                bytes,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_files_inside_the_shared_folder_are_read() {
        let base = tempfile::tempdir().expect("directory");
        let shared = base.path().join(SHARED_DIRECTORY);
        fs::create_dir_all(&shared).expect("shared");
        fs::write(shared.join("0-nota.txt"), b"ciao").expect("write");
        fs::write(base.path().join("segreto.txt"), b"no").expect("write");
        let copies = vec![
            file_access::SharedFileCopy {
                path: shared.join("0-nota.txt"),
                name: "nota.txt".to_owned(),
            },
            file_access::SharedFileCopy {
                path: shared.join("../segreto.txt"),
                name: "segreto.txt".to_owned(),
            },
        ];
        let files = read_shared_files(&shared, &copies);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "nota.txt");
        assert_eq!(files[0].bytes, b"ciao");
    }
}
