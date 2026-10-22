//! Cestino di spazi, taccuini, tag, attività ed eventi (le note hanno il
//! proprio flusso in `storage`).
//!
//! Spostare un contenitore nel cestino stacca i suoi collegamenti e li
//! registra in `trash_relations`: le note di un taccuino diventano "senza
//! taccuino", i taccuini di uno spazio "senza spazio", le note perdono il tag.
//! Nessun dato viene eliminato e il resto dell'app non vede mai riferimenti a
//! elementi nel cestino. Il ripristino ricrea i collegamenti, salvo quelli
//! che l'utente ha cambiato nel frattempo (una nota spostata altrove resta
//! dove l'ha messa).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::storage::{Database, StorageError};

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrashKind {
    Space,
    Notebook,
    Tag,
    Task,
    Event,
    Template,
}

impl TrashKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Space => "space",
            Self::Notebook => "notebook",
            Self::Tag => "tag",
            Self::Task => "task",
            Self::Event => "event",
            Self::Template => "template",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "space" => Some(Self::Space),
            "notebook" => Some(Self::Notebook),
            "tag" => Some(Self::Tag),
            "task" => Some(Self::Task),
            "event" => Some(Self::Event),
            "template" => Some(Self::Template),
            _ => None,
        }
    }

    /// Tabella dell'elemento: nomi fissi, mai input dell'utente.
    fn table(self) -> &'static str {
        match self {
            Self::Space => "spaces",
            Self::Notebook => "notebooks",
            Self::Tag => "tags",
            Self::Task => "tasks",
            Self::Event => "events",
            Self::Template => "templates",
        }
    }

    fn not_found(self) -> StorageError {
        match self {
            Self::Space => StorageError::SpaceNotFound,
            Self::Notebook => StorageError::NotebookNotFound,
            Self::Tag => StorageError::TagNotFound,
            Self::Task => StorageError::TaskNotFound,
            Self::Event => StorageError::EventNotFound,
            Self::Template => StorageError::TemplateNotFound,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashItem {
    pub kind: TrashKind,
    pub id: String,
    pub title: String,
    pub deleted_at: String,
    /// Collegamenti staccati che il ripristino ricrea (note di un taccuino,
    /// taccuini di uno spazio, note di un tag).
    pub related_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyTrashSummary {
    pub notes: u64,
    pub items: u64,
}

/// Collegamenti staccati da un contenitore: (selezione dei collegati,
/// istruzione che li stacca). `?1` è l'ID del contenitore.
fn detach_statements(kind: TrashKind) -> Option<(&'static str, &'static str)> {
    match kind {
        TrashKind::Space => Some((
            "SELECT id AS related_id FROM notebooks WHERE space_id = ?1",
            "UPDATE notebooks SET space_id = NULL WHERE space_id = ?1",
        )),
        TrashKind::Notebook => Some((
            "SELECT id AS related_id FROM notes WHERE notebook_id = ?1",
            "UPDATE notes SET notebook_id = NULL WHERE notebook_id = ?1",
        )),
        TrashKind::Tag => Some((
            "SELECT note_id AS related_id FROM note_tags WHERE tag_id = ?1",
            "DELETE FROM note_tags WHERE tag_id = ?1",
        )),
        TrashKind::Task | TrashKind::Event | TrashKind::Template => None,
    }
}

/// Istruzione che ricrea i collegamenti registrati. `?1` è l'ID del
/// contenitore; si ricollegano solo elementi ancora esistenti e non
/// riassegnati nel frattempo.
fn reattach_statement(kind: TrashKind) -> Option<&'static str> {
    match kind {
        TrashKind::Space => Some(
            "UPDATE notebooks SET space_id = ?1
             WHERE space_id IS NULL AND id IN (
                 SELECT related_id FROM trash_relations
                 WHERE item_type = 'space' AND item_id = ?1)",
        ),
        TrashKind::Notebook => Some(
            "UPDATE notes SET notebook_id = ?1
             WHERE notebook_id IS NULL AND id IN (
                 SELECT related_id FROM trash_relations
                 WHERE item_type = 'notebook' AND item_id = ?1)",
        ),
        TrashKind::Tag => Some(
            "INSERT OR IGNORE INTO note_tags (note_id, tag_id)
             SELECT related_id, ?1 FROM trash_relations
             WHERE item_type = 'tag' AND item_id = ?1
               AND EXISTS (SELECT 1 FROM notes WHERE notes.id = related_id)",
        ),
        TrashKind::Task | TrashKind::Event | TrashKind::Template => None,
    }
}

fn move_to_trash(
    transaction: &Transaction<'_>,
    kind: TrashKind,
    id: &str,
) -> Result<(), StorageError> {
    let table = kind.table();
    if kind == TrashKind::Event {
        let external: Option<Option<String>> = transaction
            .query_row(
                "SELECT calendar_provider FROM events WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if matches!(external, Some(Some(_))) {
            return Err(StorageError::ExternalEvent);
        }
    }
    let changed = transaction.execute(
        &format!(
            "UPDATE {table} SET deleted_at = {NOW}, updated_at = {NOW}
             WHERE id = ?1 AND deleted_at IS NULL"
        ),
        [id],
    )?;
    if changed == 0 {
        return Err(kind.not_found());
    }
    if let Some((select, detach)) = detach_statements(kind) {
        transaction.execute(
            &format!(
                "INSERT OR IGNORE INTO trash_relations (item_type, item_id, related_id)
                 SELECT ?2, ?1, related_id FROM ({select})"
            ),
            params![id, kind.as_str()],
        )?;
        transaction.execute(detach, [id])?;
    }
    Ok(())
}

fn ensure_trashed(connection: &Connection, kind: TrashKind, id: &str) -> Result<(), StorageError> {
    let table = kind.table();
    let trashed: bool = connection.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1 AND deleted_at IS NOT NULL)"),
        [id],
        |row| row.get(0),
    )?;
    if trashed {
        Ok(())
    } else {
        Err(StorageError::TrashItemNotFound)
    }
}

fn forget_relations(
    transaction: &Transaction<'_>,
    kind: TrashKind,
    id: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "DELETE FROM trash_relations WHERE item_type = ?1 AND item_id = ?2",
        params![kind.as_str(), id],
    )?;
    Ok(())
}

impl Database {
    /// Sposta nel cestino spazio, taccuino, tag, attività o evento.
    pub fn move_item_to_trash(&self, kind: TrashKind, id: &str) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        move_to_trash(&transaction, kind, id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Elementi nel cestino diversi dalle note, dal più recente.
    pub fn list_trash_items(&self) -> Result<Vec<TrashItem>, StorageError> {
        let connection = self.connection()?;
        let relations = "(SELECT COUNT(*) FROM trash_relations
                          WHERE item_type = items.kind AND item_id = items.id)";
        let mut statement = connection.prepare(&format!(
            "SELECT kind, id, title, deleted_at, {relations} FROM (
                 SELECT 'space' AS kind, id, name AS title, deleted_at FROM spaces
                 WHERE deleted_at IS NOT NULL
                 UNION ALL
                 SELECT 'notebook', id, name, deleted_at FROM notebooks
                 WHERE deleted_at IS NOT NULL
                 UNION ALL
                 SELECT 'tag', id, name, deleted_at FROM tags WHERE deleted_at IS NOT NULL
                 UNION ALL
                 SELECT 'task', id, title, deleted_at FROM tasks WHERE deleted_at IS NOT NULL
                 UNION ALL
                 SELECT 'event', id, title, deleted_at FROM events WHERE deleted_at IS NOT NULL
                 UNION ALL
                 SELECT 'template', id, name, deleted_at FROM templates
                 WHERE deleted_at IS NOT NULL
             ) AS items
             ORDER BY deleted_at DESC, id
             LIMIT 1000"
        ))?;
        let rows = statement.query_map([], |row| {
            let kind: String = row.get(0)?;
            Ok((
                kind,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (kind, id, title, deleted_at, related) = row?;
            if let Some(kind) = TrashKind::parse(&kind) {
                items.push(TrashItem {
                    kind,
                    id,
                    title,
                    deleted_at,
                    related_count: u64::try_from(related).unwrap_or(0),
                });
            }
        }
        Ok(items)
    }

    /// Ripristina un elemento e ricrea i collegamenti ancora validi.
    pub fn restore_trash_item(&self, kind: TrashKind, id: &str) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let table = kind.table();
        let changed = transaction.execute(
            &format!(
                "UPDATE {table} SET deleted_at = NULL, updated_at = {NOW}
                 WHERE id = ?1 AND deleted_at IS NOT NULL"
            ),
            [id],
        )?;
        if changed == 0 {
            return Err(StorageError::TrashItemNotFound);
        }
        if let Some(reattach) = reattach_statement(kind) {
            transaction.execute(reattach, [id])?;
        }
        forget_relations(&transaction, kind, id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Elimina definitivamente un elemento che si trova nel cestino. Le
    /// note e i taccuini che vi erano collegati restano (senza contenitore);
    /// i promemoria di attività ed eventi vengono eliminati dai trigger.
    pub fn delete_trash_item(&self, kind: TrashKind, id: &str) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        ensure_trashed(&transaction, kind, id)?;
        let table = kind.table();
        transaction.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [id])?;
        forget_relations(&transaction, kind, id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Svuota il cestino: note (con i loro file) ed elementi.
    pub fn empty_trash(&self, data_directory: &Path) -> Result<EmptyTrashSummary, StorageError> {
        let notes = self.purge_trashed_notes(None, data_directory)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut items = 0;
        for kind in [
            TrashKind::Task,
            TrashKind::Event,
            TrashKind::Template,
            TrashKind::Tag,
            TrashKind::Notebook,
            TrashKind::Space,
        ] {
            let table = kind.table();
            items += transaction.execute(
                &format!("DELETE FROM {table} WHERE deleted_at IS NOT NULL"),
                [],
            )? as u64;
        }
        transaction.execute("DELETE FROM trash_relations", [])?;
        transaction.commit()?;
        Ok(EmptyTrashSummary { notes, items })
    }
}

#[cfg(test)]
mod tests {
    use super::{EmptyTrashSummary, TrashKind};
    use crate::{
        domain::{
            note::empty_document,
            planner::{EventInput, ReminderEntity, TaskInput, TaskView},
        },
        storage::{Database, StorageError},
        time,
    };

    fn task(title: &str) -> TaskInput {
        TaskInput {
            title: title.to_owned(),
            description: String::new(),
            priority: 0,
            due_date: Some("2026-10-14".to_owned()),
            note_id: None,
            flagged: false,
            recurrence: None,
        }
    }

    #[test]
    fn notebook_in_trash_releases_notes_and_restores_them() {
        let database = Database::in_memory().expect("database");
        let space = database.create_space("Lavoro", "").expect("space");
        let notebook = database
            .create_notebook(Some(&space.id), "Clienti", "")
            .expect("notebook");
        let inside = database
            .create_note_in_notebook("Dentro", &empty_document(), Some(&notebook.id))
            .expect("note");
        let moved = database
            .create_note_in_notebook("Spostata", &empty_document(), Some(&notebook.id))
            .expect("note");

        database
            .move_item_to_trash(TrashKind::Notebook, &notebook.id)
            .expect("trash");
        assert!(database.list_notebooks().expect("notebooks").is_empty());
        // Le note restano accessibili, senza taccuino.
        assert!(
            database
                .get_note(&inside.id)
                .expect("note")
                .notebook_id
                .is_none()
        );
        let unfiled = database
            .search_notes("", "unfiled", None, 10, 0)
            .expect("unfiled");
        assert_eq!(unfiled.len(), 2);
        // Un taccuino nel cestino non accetta note.
        assert!(matches!(
            database.move_note(&moved.id, Some(&notebook.id)),
            Err(StorageError::NotebookNotFound)
        ));
        assert!(matches!(
            database.create_note_in_notebook("x", &empty_document(), Some(&notebook.id)),
            Err(StorageError::NotebookNotFound)
        ));
        let other = database
            .create_notebook(None, "Altro", "")
            .expect("notebook");
        database
            .move_note(&moved.id, Some(&other.id))
            .expect("move");

        let items = database.list_trash_items().expect("items");
        assert_eq!(items.len(), 1);
        assert_eq!(
            (items[0].kind, items[0].related_count),
            (TrashKind::Notebook, 2)
        );

        database
            .restore_trash_item(TrashKind::Notebook, &notebook.id)
            .expect("restore");
        assert_eq!(
            database
                .get_note(&inside.id)
                .expect("note")
                .notebook_id
                .as_deref(),
            Some(notebook.id.as_str())
        );
        // La nota spostata nel frattempo resta dove l'utente l'ha messa.
        assert_eq!(
            database
                .get_note(&moved.id)
                .expect("note")
                .notebook_id
                .as_deref(),
            Some(other.id.as_str())
        );
        assert!(database.list_trash_items().expect("items").is_empty());
    }

    #[test]
    fn spaces_and_tags_go_to_trash_and_come_back() {
        let database = Database::in_memory().expect("database");
        let space = database.create_space("Casa", "").expect("space");
        database
            .create_notebook(Some(&space.id), "Ricette", "")
            .expect("notebook");
        let note = database
            .create_note("Pasta", &empty_document())
            .expect("note");
        let tag = database.create_tag("cucina", None).expect("tag");
        database
            .set_note_tags(&note.id, std::slice::from_ref(&tag.id))
            .expect("tags");

        database
            .move_item_to_trash(TrashKind::Space, &space.id)
            .expect("trash space");
        database
            .move_item_to_trash(TrashKind::Tag, &tag.id)
            .expect("trash tag");
        assert!(database.list_spaces().expect("spaces").is_empty());
        assert!(database.list_notebooks().expect("nb")[0].space_id.is_none());
        assert!(database.list_tags().expect("tags").is_empty());
        assert!(
            database
                .list_note_tags(&note.id)
                .expect("note tags")
                .is_empty()
        );
        assert!(
            database
                .search_notes("tag:cucina", "all", None, 10, 0)
                .expect("search")
                .is_empty()
        );
        assert!(matches!(
            database.create_notebook(Some(&space.id), "x", ""),
            Err(StorageError::SpaceNotFound)
        ));
        // Il nome del tag nel cestino non è riutilizzabile per errore.
        assert!(matches!(
            database.create_tag("Cucina", None),
            Err(StorageError::TagNameInTrash)
        ));
        assert!(matches!(
            database.move_item_to_trash(TrashKind::Tag, &tag.id),
            Err(StorageError::TagNotFound)
        ));

        database
            .restore_trash_item(TrashKind::Space, &space.id)
            .expect("restore space");
        database
            .restore_trash_item(TrashKind::Tag, &tag.id)
            .expect("restore tag");
        assert_eq!(
            database.list_notebooks().expect("nb")[0]
                .space_id
                .as_deref(),
            Some(space.id.as_str())
        );
        assert_eq!(
            database.list_note_tags(&note.id).expect("note tags")[0].id,
            tag.id
        );
        assert_eq!(
            database
                .search_notes("tag:cucina", "all", None, 10, 0)
                .expect("search")
                .len(),
            1
        );
        assert!(matches!(
            database.restore_trash_item(TrashKind::Tag, &tag.id),
            Err(StorageError::TrashItemNotFound)
        ));
        assert!(matches!(
            database.create_tag("CUCINA", None),
            Err(StorageError::DuplicateTagName)
        ));
    }

    #[test]
    fn tasks_and_events_in_trash_are_hidden_and_silent() {
        let database = Database::in_memory().expect("database");
        let now = time::parse_utc("2026-10-14T06:00:00Z").expect("now");
        let today = time::parse_date("2026-10-14").expect("date");
        let task_item = database.create_task(&task("Nel cestino")).expect("task");
        database
            .create_reminder(
                ReminderEntity::Task,
                &task_item.id,
                "2026-10-14T09:00",
                Some("Europe/Rome"),
                now,
            )
            .expect("reminder");
        let event = database
            .create_event(&EventInput {
                title: "Evento".to_owned(),
                description: String::new(),
                all_day: true,
                start: "2026-10-14".to_owned(),
                end: "2026-10-14".to_owned(),
                timezone: Some("Europe/Rome".to_owned()),
                note_id: None,
            })
            .expect("event");

        database
            .move_item_to_trash(TrashKind::Task, &task_item.id)
            .expect("trash task");
        database
            .move_item_to_trash(TrashKind::Event, &event.id)
            .expect("trash event");
        let rome = time::Zone::resolve(Some("Europe/Rome")).expect("tz");
        assert!(
            database
                .list_tasks(TaskView::Open, today, None)
                .expect("tasks")
                .is_empty()
        );
        assert_eq!(database.task_counts(today).expect("counts").today, 0);
        assert!(
            database
                .list_tasks_due_between(today, today)
                .expect("due")
                .is_empty()
        );
        assert!(
            database
                .list_events(today, today, &rome)
                .expect("events")
                .is_empty()
        );
        let later = time::parse_utc("2026-10-14T08:00:00Z").expect("later");
        assert!(database.take_due_reminders(later).expect("due").is_empty());
        assert!(database.list_reminders().expect("reminders").is_empty());

        database
            .restore_trash_item(TrashKind::Task, &task_item.id)
            .expect("restore");
        assert_eq!(
            database
                .list_tasks(TaskView::Today, today, None)
                .expect("tasks")
                .len(),
            1
        );
        assert_eq!(database.take_due_reminders(later).expect("due").len(), 1);

        // Eliminazione definitiva solo dal cestino.
        assert!(matches!(
            database.delete_trash_item(TrashKind::Task, &task_item.id),
            Err(StorageError::TrashItemNotFound)
        ));
        database
            .delete_trash_item(TrashKind::Event, &event.id)
            .expect("delete event");
        assert!(matches!(
            database.get_event(&event.id),
            Err(StorageError::EventNotFound)
        ));
    }

    #[test]
    fn empty_trash_removes_notes_and_items_only() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database");
        let notebook = database
            .create_notebook(None, "Vecchio", "")
            .expect("notebook");
        let kept_note = database
            .create_note_in_notebook("Resta", &empty_document(), Some(&notebook.id))
            .expect("note");
        let trashed_note = database
            .create_note("Via", &empty_document())
            .expect("note");
        database.trash_note(&trashed_note.id).expect("trash note");
        database
            .move_item_to_trash(TrashKind::Notebook, &notebook.id)
            .expect("trash notebook");
        let tag = database.create_tag("vecchio", None).expect("tag");
        database
            .move_item_to_trash(TrashKind::Tag, &tag.id)
            .expect("trash tag");
        let active_task = database.create_task(&task("Attiva")).expect("task");

        assert_eq!(
            database.empty_trash(data).expect("empty"),
            EmptyTrashSummary { notes: 1, items: 2 }
        );
        assert!(database.list_trash_items().expect("items").is_empty());
        // La nota del taccuino eliminato resta, senza taccuino.
        assert!(
            database
                .get_note(&kept_note.id)
                .expect("note")
                .notebook_id
                .is_none()
        );
        assert!(database.get_task(&active_task.id).is_ok());
        // Il nome del tag eliminato è di nuovo libero.
        database
            .create_tag("vecchio", None)
            .expect("name available");
        assert!(database.check_integrity().expect("integrity").ok);
    }
}
