//! Preferenze degli aggiornamenti dell'app (ADR 0023). Il controllo, il
//! download e l'installazione sono nella shell desktop (plugin updater di
//! Tauri); qui resta solo ciò che va salvato nel database.

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::storage::{Database, StorageError};

/// Chiave della tabella `settings`.
pub const UPDATE_SETTINGS_KEY: &str = "updates";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Cerca e scarica gli aggiornamenti in background; vengono installati
    /// alla chiusura dell'app o con «Riavvia e aggiorna».
    pub auto_update: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto_update: true }
    }
}

impl Database {
    pub fn update_settings(&self) -> Result<UpdateSettings, StorageError> {
        let stored: Option<String> = self
            .connection()?
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [UPDATE_SETTINGS_KEY],
                |row| row.get(0),
            )
            .optional()?;
        // Un valore illeggibile torna ai predefiniti (aggiornamenti attivi).
        Ok(stored
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default())
    }

    pub fn set_update_settings(
        &self,
        settings: &UpdateSettings,
    ) -> Result<UpdateSettings, StorageError> {
        let value = serde_json::to_string(settings)?;
        self.connection()?.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            [UPDATE_SETTINGS_KEY, value.as_str()],
        )?;
        Ok(settings.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_automatic_updates_and_persist() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("gna.sqlite3");
        let database = Database::open(&path).expect("database");
        assert!(database.update_settings().expect("settings").auto_update);
        database
            .set_update_settings(&UpdateSettings { auto_update: false })
            .expect("save");
        drop(database);
        let reopened = Database::open(&path).expect("reopen");
        assert!(!reopened.update_settings().expect("settings").auto_update);

        // Un valore danneggiato non blocca l'app.
        reopened
            .connection()
            .expect("connection")
            .execute(
                "UPDATE settings SET value = 'x' WHERE key = ?1",
                [UPDATE_SETTINGS_KEY],
            )
            .expect("corrupt");
        assert!(reopened.update_settings().expect("settings").auto_update);
    }
}
