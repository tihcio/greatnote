//! Preferenze della modalità in background del desktop (ADR 0031): l'icona
//! nell'area di notifica, la finestra nascosta alla chiusura e l'avvio
//! all'accesso. Il comportamento è nella shell desktop; qui resta ciò che va
//! salvato nel database.

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::storage::{Database, StorageError};

/// Chiave della tabella `settings`. Le preferenze riguardano questo
/// computer: la tabella non è sincronizzata.
pub const BACKGROUND_SETTINGS_KEY: &str = "background";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundSettings {
    /// Chiudendo la finestra Greatnote resta attivo nell'area di notifica,
    /// così i promemoria arrivano anche senza finestra aperta.
    pub enabled: bool,
    /// Avvio all'accesso, con la finestra nascosta. Richiede `enabled`.
    pub autostart: bool,
}

impl BackgroundSettings {
    /// L'avvio all'accesso ha senso solo con la modalità in background.
    pub fn normalized(mut self) -> Self {
        self.autostart &= self.enabled;
        self
    }
}

impl Database {
    pub fn background_settings(&self) -> Result<BackgroundSettings, StorageError> {
        let stored: Option<String> = self
            .connection()?
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [BACKGROUND_SETTINGS_KEY],
                |row| row.get(0),
            )
            .optional()?;
        // Un valore illeggibile torna ai predefiniti (modalità spenta).
        Ok(stored
            .and_then(|value| serde_json::from_str::<BackgroundSettings>(&value).ok())
            .unwrap_or_default()
            .normalized())
    }

    pub fn set_background_settings(
        &self,
        settings: &BackgroundSettings,
    ) -> Result<BackgroundSettings, StorageError> {
        let settings = settings.clone().normalized();
        let value = serde_json::to_string(&settings)?;
        self.connection()?.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            [BACKGROUND_SETTINGS_KEY, value.as_str()],
        )?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_off_persist_and_need_background_for_autostart() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("gna.sqlite3");
        let database = Database::open(&path).expect("database");
        assert_eq!(
            database.background_settings().expect("settings"),
            BackgroundSettings::default()
        );
        // L'avvio all'accesso senza modalità in background viene tolto.
        let saved = database
            .set_background_settings(&BackgroundSettings {
                enabled: false,
                autostart: true,
            })
            .expect("save");
        assert!(!saved.autostart);
        database
            .set_background_settings(&BackgroundSettings {
                enabled: true,
                autostart: true,
            })
            .expect("save");
        drop(database);
        let reopened = Database::open(&path).expect("reopen");
        assert_eq!(
            reopened.background_settings().expect("settings"),
            BackgroundSettings {
                enabled: true,
                autostart: true
            }
        );
        reopened
            .connection()
            .expect("connection")
            .execute(
                "UPDATE settings SET value = 'x' WHERE key = 'background'",
                [],
            )
            .expect("corrupt");
        assert_eq!(
            reopened.background_settings().expect("settings"),
            BackgroundSettings::default()
        );
    }
}
