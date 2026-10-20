//! Spostamento dei dati dalla cartella usata prima del cambio di nome.
//!
//! La cartella dati nuova può esistere già prima che il database venga
//! aperto (il motore web vi crea le proprie cache), quindi la decisione si
//! basa sul database e non sull'esistenza della cartella. Vengono spostati
//! solo i file dell'app; le cache del motore web restano nella cartella
//! precedente.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags};
use uuid::Uuid;

use crate::{backup::private_directory, storage::StorageError};

pub const DATABASE_FILE: &str = "gna.sqlite3";
/// Voci appartenenti all'app, nell'ordine in cui vengono spostate.
const DATA_ENTRIES: [&str; 6] = [
    DATABASE_FILE,
    "gna.sqlite3-journal",
    "gna.sqlite3-wal",
    "gna.sqlite3-shm",
    "attachments",
    "backups",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyMigration {
    /// Nessun database nella cartella precedente.
    NotNeeded,
    /// Entrambe le cartelle contengono dati: nessuna modifica.
    BothContainData,
    /// Dati spostati; l'eventuale database vuoto è conservato in `set_aside`.
    Moved { set_aside: Option<PathBuf> },
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

/// Vero solo se il database non contiene alcun dato dell'utente. In caso di
/// dubbio (errore di lettura, schema inatteso) restituisce falso.
fn is_pristine_database(path: &Path) -> bool {
    let Ok(connection) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return false;
    };
    ["notes", "notebooks", "spaces", "tags", "note_drafts"]
        .iter()
        .all(|table| {
            connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .is_ok_and(|count| count == 0)
        })
}

/// Sposta database, allegati e backup da `legacy` a `current` quando
/// `current` non contiene ancora dati dell'utente. Un database nuovo ma vuoto
/// viene spostato in una sottocartella `.replaced-<id>` invece di essere
/// eliminato. Se uno spostamento fallisce, quelli già eseguiti vengono
/// annullati e l'errore viene restituito.
pub fn migrate_legacy_data_directory(
    legacy: &Path,
    current: &Path,
) -> Result<LegacyMigration, StorageError> {
    if legacy == current || !is_regular_file(&legacy.join(DATABASE_FILE)) {
        return Ok(LegacyMigration::NotNeeded);
    }
    let current_database = current.join(DATABASE_FILE);
    let current_has_database = fs::symlink_metadata(&current_database).is_ok();
    if current_has_database
        && !(is_regular_file(&current_database) && is_pristine_database(&current_database))
    {
        return Ok(LegacyMigration::BothContainData);
    }
    private_directory(current)?;

    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| -> Result<Option<PathBuf>, StorageError> {
        let set_aside = if current_has_database {
            let aside = current.join(format!(".replaced-{}", Uuid::new_v4().simple()));
            private_directory(&aside)?;
            for entry in DATA_ENTRIES {
                let source = current.join(entry);
                if fs::symlink_metadata(&source).is_ok() {
                    let target = aside.join(entry);
                    fs::rename(&source, &target)?;
                    moved.push((source, target));
                }
            }
            Some(aside)
        } else {
            for entry in DATA_ENTRIES {
                if fs::symlink_metadata(current.join(entry)).is_ok() {
                    return Err(StorageError::Io(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("la cartella dati contiene già {entry}"),
                    )));
                }
            }
            None
        };
        for entry in DATA_ENTRIES {
            let source = legacy.join(entry);
            if fs::symlink_metadata(&source).is_ok() {
                let target = current.join(entry);
                fs::rename(&source, &target)?;
                moved.push((source, target));
            }
        }
        Ok(set_aside)
    })();

    match result {
        Ok(set_aside) => Ok(LegacyMigration::Moved { set_aside }),
        Err(error) => {
            for (source, target) in moved.iter().rev() {
                if let Err(undo) = fs::rename(target, source) {
                    tracing::error!("annullamento dello spostamento non riuscito: {undo}");
                }
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{DATABASE_FILE, LegacyMigration, migrate_legacy_data_directory};
    use crate::{domain::note::empty_document, storage::Database};

    fn database_with_note(directory: &std::path::Path, title: &str) {
        let database = Database::open(directory.join(DATABASE_FILE)).expect("database opens");
        let note = database
            .create_note(title, &empty_document())
            .expect("note");
        database
            .add_attachment(&note.id, "a.txt", b"allegato", directory)
            .expect("attachment");
        database
            .create_backup(crate::backup::BackupKind::Manual)
            .expect("backup");
    }

    #[test]
    fn moves_legacy_data_when_current_directory_holds_only_web_caches() {
        let root = tempfile::tempdir().expect("temporary directory");
        let legacy = root.path().join("dev.gna.notes");
        let current = root.path().join("dev.greatnote.notes");
        fs::create_dir(&legacy).expect("legacy");
        database_with_note(&legacy, "Vecchia");
        fs::create_dir_all(current.join("WebKitCache")).expect("web cache");
        fs::create_dir_all(legacy.join("WebKitCache")).expect("legacy web cache");

        assert_eq!(
            migrate_legacy_data_directory(&legacy, &current).expect("migration"),
            LegacyMigration::Moved { set_aside: None }
        );
        let database = Database::open(current.join(DATABASE_FILE)).expect("reopen");
        let notes = database.list_notes(10, 0).expect("notes");
        assert_eq!(notes[0].title, "Vecchia");
        let attachments = database
            .list_attachments(&notes[0].id)
            .expect("attachments");
        database
            .verified_attachment_path(&notes[0].id, &attachments[0].id, &current)
            .expect("attachment moved");
        assert_eq!(database.list_backups().expect("backups").len(), 1);
        assert!(!legacy.join(DATABASE_FILE).exists());
        assert!(legacy.join("WebKitCache").exists());

        // Una seconda esecuzione non fa nulla.
        assert_eq!(
            migrate_legacy_data_directory(&legacy, &current).expect("again"),
            LegacyMigration::NotNeeded
        );
    }

    #[test]
    fn replaces_an_empty_new_database_but_keeps_it_aside() {
        let root = tempfile::tempdir().expect("temporary directory");
        let legacy = root.path().join("dev.gna.notes");
        let current = root.path().join("dev.greatnote.notes");
        fs::create_dir(&legacy).expect("legacy");
        fs::create_dir(&current).expect("current");
        database_with_note(&legacy, "Vecchia");
        {
            let empty = Database::open(current.join(DATABASE_FILE)).expect("empty database");
            empty.run_automatic_backup().expect("automatic backup");
        }

        let LegacyMigration::Moved {
            set_aside: Some(aside),
        } = migrate_legacy_data_directory(&legacy, &current).expect("migration")
        else {
            panic!("the empty database must be set aside");
        };
        assert!(aside.join(DATABASE_FILE).exists());
        assert!(aside.join("backups").exists());
        let database = Database::open(current.join(DATABASE_FILE)).expect("reopen");
        assert_eq!(
            database.list_notes(10, 0).expect("notes")[0].title,
            "Vecchia"
        );
    }

    #[test]
    fn never_touches_data_when_both_directories_have_notes() {
        let root = tempfile::tempdir().expect("temporary directory");
        let legacy = root.path().join("dev.gna.notes");
        let current = root.path().join("dev.greatnote.notes");
        fs::create_dir(&legacy).expect("legacy");
        fs::create_dir(&current).expect("current");
        database_with_note(&legacy, "Vecchia");
        database_with_note(&current, "Nuova");

        assert_eq!(
            migrate_legacy_data_directory(&legacy, &current).expect("migration"),
            LegacyMigration::BothContainData
        );
        assert!(legacy.join(DATABASE_FILE).exists());
        let database = Database::open(current.join(DATABASE_FILE)).expect("reopen");
        assert_eq!(database.list_notes(10, 0).expect("notes")[0].title, "Nuova");
    }
}
