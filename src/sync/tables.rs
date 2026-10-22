//! Tabelle sincronizzate: chiavi, colonne, filtri e riferimenti. Le
//! istantanee sono oggetti JSON con le sole colonne elencate; nessun nome di
//! colonna o tabella arriva mai dai dati remoti (le query sono costruite
//! solo da queste costanti).

use rusqlite::{Connection, OptionalExtension, ToSql, types::Value as SqlValue};
use serde_json::{Map, Number, Value};

use crate::storage::StorageError;

/// Cosa fare se un riferimento punta a un'entità che qui non esiste (per
/// esempio eliminata definitivamente): come i vincoli dello schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Missing {
    SetNull,
    Skip,
}

pub(crate) struct Reference {
    pub column: &'static str,
    pub table: &'static str,
    pub missing: Missing,
}

pub(crate) struct SyncTable {
    /// Nome dell'entità nel protocollo, uguale al nome della tabella.
    pub name: &'static str,
    pub keys: &'static [&'static str],
    /// Colonne non chiave sincronizzate.
    pub columns: &'static [&'static str],
    /// Righe escluse dalla sincronizzazione (condizione SQL senza alias).
    pub filter: Option<&'static str>,
    pub references: &'static [Reference],
    /// Le versioni perdenti dei conflitti vengono conservate per l'utente.
    pub keep_conflicts: bool,
}

const fn reference(column: &'static str, table: &'static str, missing: Missing) -> Reference {
    Reference {
        column,
        table,
        missing,
    }
}

/// In ordine di dipendenza: i genitori prima dei figli.
pub(crate) const TABLES: &[SyncTable] = &[
    SyncTable {
        name: "spaces",
        keys: &["id"],
        columns: &[
            "name",
            "description",
            "sort_order",
            "created_at",
            "updated_at",
            "deleted_at",
        ],
        filter: None,
        references: &[],
        keep_conflicts: false,
    },
    SyncTable {
        name: "notebooks",
        keys: &["id"],
        columns: &[
            "space_id",
            "name",
            "description",
            "sort_order",
            "created_at",
            "updated_at",
            "deleted_at",
        ],
        filter: None,
        references: &[reference("space_id", "spaces", Missing::SetNull)],
        keep_conflicts: false,
    },
    SyncTable {
        name: "tags",
        keys: &["id"],
        columns: &["name", "color", "created_at", "updated_at", "deleted_at"],
        filter: None,
        references: &[],
        keep_conflicts: false,
    },
    SyncTable {
        name: "notes",
        keys: &["id"],
        columns: &[
            "notebook_id",
            "title",
            "content_json",
            "content_version",
            "is_pinned",
            "created_at",
            "updated_at",
            "archived_at",
            "deleted_at",
            "source",
        ],
        filter: None,
        references: &[reference("notebook_id", "notebooks", Missing::SetNull)],
        keep_conflicts: true,
    },
    SyncTable {
        name: "note_tags",
        keys: &["note_id", "tag_id"],
        columns: &[],
        filter: None,
        references: &[
            reference("note_id", "notes", Missing::Skip),
            reference("tag_id", "tags", Missing::Skip),
        ],
        keep_conflicts: false,
    },
    SyncTable {
        name: "note_links",
        keys: &["source_note_id", "target_note_id"],
        columns: &["label", "created_at"],
        filter: None,
        references: &[
            reference("source_note_id", "notes", Missing::Skip),
            reference("target_note_id", "notes", Missing::Skip),
        ],
        keep_conflicts: false,
    },
    SyncTable {
        name: "attachments",
        keys: &["id"],
        columns: &[
            "note_id",
            "original_filename",
            "stored_filename",
            "mime_type",
            "size_bytes",
            "sha256",
            "created_at",
            "deleted_at",
            "search_text",
            "ocr_text",
            "ocr_status",
            "ocr_engine",
            "ocr_error",
            "ocr_at",
        ],
        filter: None,
        references: &[reference("note_id", "notes", Missing::Skip)],
        keep_conflicts: false,
    },
    SyncTable {
        name: "tasks",
        keys: &["id"],
        columns: &[
            "note_id",
            "title",
            "description",
            "status",
            "priority",
            "due_date",
            "completed_at",
            "created_at",
            "updated_at",
            "deleted_at",
            "flagged",
            "recurrence",
        ],
        filter: None,
        references: &[reference("note_id", "notes", Missing::SetNull)],
        keep_conflicts: false,
    },
    SyncTable {
        name: "events",
        keys: &["id"],
        columns: &[
            "note_id",
            "title",
            "description",
            "all_day",
            "start_at",
            "end_at",
            "start_date",
            "end_date",
            "timezone",
            "created_at",
            "updated_at",
            "deleted_at",
        ],
        // Gli eventi di calendari esterni e iCal restano locali.
        filter: Some("calendar_provider IS NULL"),
        references: &[reference("note_id", "notes", Missing::SetNull)],
        keep_conflicts: false,
    },
    SyncTable {
        name: "reminders",
        keys: &["id"],
        columns: &[
            "entity_type",
            "entity_id",
            "remind_at",
            "timezone",
            "local_time",
            "status",
            "last_triggered_at",
            "created_at",
            "updated_at",
        ],
        filter: Some(
            "(entity_type <> 'event' OR EXISTS (SELECT 1 FROM events
               WHERE events.id = reminders.entity_id AND events.calendar_provider IS NULL))",
        ),
        references: &[],
        keep_conflicts: false,
    },
    SyncTable {
        name: "templates",
        keys: &["id"],
        columns: &[
            "name",
            "title",
            "content_json",
            "created_at",
            "updated_at",
            "deleted_at",
        ],
        filter: None,
        references: &[],
        keep_conflicts: true,
    },
    SyncTable {
        name: "trash_relations",
        keys: &["item_type", "item_id", "related_id"],
        columns: &[],
        filter: None,
        references: &[],
        keep_conflicts: false,
    },
];

pub(crate) fn table(name: &str) -> Option<&'static SyncTable> {
    TABLES.iter().find(|table| table.name == name)
}

/// Chiave composta in forma di testo (`a|b`), come nei trigger.
pub(crate) fn split_key<'a>(table: &SyncTable, key: &'a str) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = key.split('|').collect();
    (parts.len() == table.keys.len()
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.len() <= 200))
    .then_some(parts)
}

fn where_keys(table: &SyncTable) -> String {
    table
        .keys
        .iter()
        .enumerate()
        .map(|(index, key)| format!("{key} = ?{}", index + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn sql_to_json(value: SqlValue) -> Value {
    match value {
        SqlValue::Null | SqlValue::Blob(_) => Value::Null,
        SqlValue::Integer(number) => Value::Number(number.into()),
        SqlValue::Real(number) => Number::from_f64(number).map_or(Value::Null, Value::Number),
        SqlValue::Text(text) => Value::String(text),
    }
}

fn json_to_sql(value: &Value) -> Option<SqlValue> {
    Some(match value {
        Value::Null => SqlValue::Null,
        Value::Bool(flag) => SqlValue::Integer(i64::from(*flag)),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => SqlValue::Integer(integer),
            None => SqlValue::Real(number.as_f64()?),
        },
        Value::String(text) => SqlValue::Text(text.clone()),
        // Le colonne sincronizzate non contengono strutture.
        Value::Array(_) | Value::Object(_) => return None,
    })
}

/// Istantanea della riga (solo colonne sincronizzate); `None` se la riga non
/// esiste o è esclusa dal filtro.
pub(crate) fn snapshot(
    connection: &Connection,
    table: &SyncTable,
    key: &str,
) -> Result<Option<Value>, StorageError> {
    let Some(parts) = split_key(table, key) else {
        return Ok(None);
    };
    let columns: Vec<&str> = table.keys.iter().chain(table.columns).copied().collect();
    let filter = table
        .filter
        .map(|filter| format!(" AND {filter}"))
        .unwrap_or_default();
    let sql = format!(
        "SELECT {} FROM {} WHERE {}{filter}",
        columns.join(", "),
        table.name,
        where_keys(table)
    );
    let params: Vec<&dyn ToSql> = parts.iter().map(|part| part as &dyn ToSql).collect();
    let row = connection
        .query_row(&sql, params.as_slice(), |row| {
            let mut object = Map::new();
            for (index, column) in columns.iter().enumerate() {
                object.insert(
                    (*column).to_owned(),
                    sql_to_json(row.get::<_, SqlValue>(index)?),
                );
            }
            Ok(Value::Object(object))
        })
        .optional()?;
    Ok(row)
}

/// Chiavi di tutte le righe sincronizzabili (prima attivazione).
pub(crate) fn all_keys(
    connection: &Connection,
    table: &SyncTable,
) -> Result<Vec<String>, StorageError> {
    let filter = table
        .filter
        .map(|filter| format!(" WHERE {filter}"))
        .unwrap_or_default();
    let key = table.keys.join(" || '|' || ");
    let mut statement = connection.prepare(&format!("SELECT {key} FROM {}{filter}", table.name))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Esito dell'applicazione di un'istantanea remota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Applied {
    Stored,
    /// Scartata per un riferimento mancante (come un CASCADE).
    Skipped,
}

/// Inserisce o aggiorna la riga con i valori dell'istantanea. Le chiavi
/// vengono dalla chiave dell'operazione, non dai dati.
///
/// Una colonna assente dall'istantanea (dispositivo con una versione
/// precedente, che non la conosce) non viene toccata: in inserimento vale il
/// default dello schema, in aggiornamento resta il valore locale. Un `null`
/// esplicito invece la svuota.
pub(crate) fn upsert(
    connection: &Connection,
    table: &SyncTable,
    key: &str,
    data: &Value,
) -> Result<Applied, StorageError> {
    let invalid = || StorageError::InvalidEvent("modifica sincronizzata non valida");
    let parts = split_key(table, key).ok_or_else(invalid)?;
    let object = data.as_object().ok_or_else(invalid)?;
    let mut values: Vec<SqlValue> = parts
        .iter()
        .map(|part| SqlValue::Text((*part).to_owned()))
        .collect();
    let columns: Vec<&str> = table
        .columns
        .iter()
        .copied()
        .filter(|column| object.contains_key(*column))
        .collect();
    for column in &columns {
        values.push(json_to_sql(&object[*column]).ok_or_else(invalid)?);
    }
    // Riferimenti a entità assenti: NULL o scarto, come i vincoli dello schema.
    let all_columns: Vec<&str> = table
        .keys
        .iter()
        .copied()
        .chain(columns.iter().copied())
        .collect();
    for reference in table.references {
        // Colonna assente: in aggiornamento resta quella locale, già valida.
        let Some(index) = all_columns
            .iter()
            .position(|column| *column == reference.column)
        else {
            continue;
        };
        let SqlValue::Text(target) = &values[index] else {
            continue;
        };
        let exists: bool = connection.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE id = ?1)",
                reference.table
            ),
            [target],
            |row| row.get(0),
        )?;
        if !exists {
            match reference.missing {
                Missing::SetNull => values[index] = SqlValue::Null,
                Missing::Skip => return Ok(Applied::Skipped),
            }
        }
    }
    let placeholders = (1..=all_columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let action = if columns.is_empty() {
        "NOTHING".to_owned()
    } else {
        format!(
            "UPDATE SET {}",
            columns
                .iter()
                .map(|column| format!("{column} = excluded.{column}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({placeholders}) ON CONFLICT ({}) DO {action}",
        table.name,
        all_columns.join(", "),
        table.keys.join(", ")
    );
    let params: Vec<&dyn ToSql> = values.iter().map(|value| value as &dyn ToSql).collect();
    connection.execute(&sql, params.as_slice())?;
    Ok(Applied::Stored)
}

pub(crate) fn delete(
    connection: &Connection,
    table: &SyncTable,
    key: &str,
) -> Result<(), StorageError> {
    let Some(parts) = split_key(table, key) else {
        return Ok(());
    };
    let params: Vec<&dyn ToSql> = parts.iter().map(|part| part as &dyn ToSql).collect();
    connection.execute(
        &format!("DELETE FROM {} WHERE {}", table.name, where_keys(table)),
        params.as_slice(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use serde_json::json;

    #[test]
    fn every_column_exists_in_the_schema() {
        let database = Database::in_memory().expect("db");
        let connection = database.connection().expect("connection");
        for table in TABLES {
            let columns: Vec<String> = connection
                .prepare(&format!(
                    "SELECT name FROM pragma_table_info('{}')",
                    table.name
                ))
                .expect("pragma")
                .query_map([], |row| row.get(0))
                .expect("rows")
                .collect::<Result<_, _>>()
                .expect("columns");
            for column in table.keys.iter().chain(table.columns) {
                assert!(
                    columns.iter().any(|name| name == column),
                    "{}.{column}",
                    table.name
                );
            }
            // Ogni tabella sincronizzata ha i trigger di registrazione.
            let triggers: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE ?1",
                    [format!("sync_{}_%", table.name)],
                    |row| row.get(0),
                )
                .expect("triggers");
            assert_eq!(triggers, 3, "{}", table.name);
        }
    }

    /// Un dispositivo con una versione precedente non conosce le colonne
    /// nuove (es. `recurrence`): le sue modifiche non le cancellano.
    #[test]
    fn absent_columns_keep_the_local_value() {
        let database = Database::in_memory().expect("db");
        let connection = database.connection().expect("connection");
        connection
            .execute(
                "INSERT INTO tasks (id, title, due_date, recurrence)
                 VALUES ('t1', 'Bollette', '2026-10-22', '{\"frequency\":\"monthly\"}')",
                [],
            )
            .expect("task");
        let tasks = table("tasks").expect("tasks");
        let mut old = snapshot(&connection, tasks, "t1")
            .expect("snapshot")
            .expect("row");
        old.as_object_mut().expect("object").remove("recurrence");
        old["title"] = json!("Bollette luce");
        upsert(&connection, tasks, "t1", &old).expect("upsert");
        let after = snapshot(&connection, tasks, "t1")
            .expect("snapshot")
            .expect("row");
        assert_eq!(after["title"], "Bollette luce");
        assert_eq!(after["recurrence"], "{\"frequency\":\"monthly\"}");
        // Una riga nuova senza la colonna prende il default dello schema.
        upsert(&connection, tasks, "t2", &old).expect("insert");
        let fresh = snapshot(&connection, tasks, "t2")
            .expect("snapshot")
            .expect("row");
        assert_eq!(fresh["recurrence"], Value::Null);
        // Un null esplicito invece la svuota.
        let mut cleared = after.clone();
        cleared["recurrence"] = Value::Null;
        upsert(&connection, tasks, "t1", &cleared).expect("upsert");
        let after = snapshot(&connection, tasks, "t1")
            .expect("snapshot")
            .expect("row");
        assert_eq!(after["recurrence"], Value::Null);
    }

    #[test]
    fn snapshots_round_trip_and_references_follow_the_schema() {
        let database = Database::in_memory().expect("db");
        let note = database
            .create_note("Titolo", &json!({"type": "doc", "content": []}))
            .expect("note");
        let connection = database.connection().expect("connection");
        let notes = table("notes").expect("notes");
        let data = snapshot(&connection, notes, &note.id)
            .expect("snapshot")
            .expect("row");
        assert_eq!(data["title"], "Titolo");
        assert_eq!(data["is_pinned"], 0);
        let mut changed = data.clone();
        changed["title"] = json!("Cambiato");
        changed["notebook_id"] = json!("inesistente");
        assert_eq!(
            upsert(&connection, notes, &note.id, &changed).expect("upsert"),
            Applied::Stored
        );
        let after = snapshot(&connection, notes, &note.id)
            .expect("snapshot")
            .expect("row");
        assert_eq!(after["title"], "Cambiato");
        assert_eq!(after["notebook_id"], Value::Null);
        // Tag della nota per una nota assente: scartato.
        let note_tags = table("note_tags").expect("table");
        assert_eq!(
            upsert(&connection, note_tags, "manca|anche", &json!({})).expect("upsert"),
            Applied::Skipped
        );
        // Dati malformati rifiutati, chiavi sbagliate ignorate.
        assert!(upsert(&connection, notes, &note.id, &json!({"title": {"x": 1}})).is_err());
        assert!(split_key(notes, "a|b").is_none());
        delete(&connection, notes, &note.id).expect("delete");
        assert!(
            snapshot(&connection, notes, &note.id)
                .expect("snapshot")
                .is_none()
        );
    }
}
