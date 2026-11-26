//! Più dispositivi simulati: database su file distinti che condividono una
//! cartella temporanea, come farebbe un servizio di sincronizzazione.

use std::{fs, path::Path};

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::domain::planner::{EventInput, ReminderEntity, TaskInput};

struct Device {
    _directory: TempDir,
    database: Database,
}

fn device(shared: &Path, name: &str) -> Device {
    let directory = tempfile::tempdir().expect("dir");
    let database = Database::open(directory.path().join("gna.sqlite3")).expect("open");
    database
        .set_sync_settings(SyncSettings {
            enabled: true,
            transport: Default::default(),
            dropbox_account: String::new(),
            folder: shared.display().to_string(),
            device_name: name.to_owned(),
            encrypted: false,
        })
        .expect("settings");
    Device {
        _directory: directory,
        database,
    }
}

fn doc(text: &str) -> Value {
    json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}]})
}

fn sync(device: &Device) -> SyncReport {
    device.database.sync_now().expect("sync")
}

fn titles(device: &Device) -> Vec<String> {
    let mut titles: Vec<String> = device
        .database
        .search_notes("", "all", None, 100, 0)
        .expect("notes")
        .into_iter()
        .map(|result| result.note.title)
        .collect();
    titles.sort();
    titles
}

fn note_text(device: &Device, id: &str) -> String {
    let note = device.database.get_note(id).expect("note");
    format!(
        "{}: {}",
        note.title,
        crate::content::to_plain_text(&note.content)
    )
}

#[test]
fn library_round_trip_is_complete_and_idempotent() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "Portatile");
    let b = device(shared.path(), "Fisso");

    let space = a.database.create_space("Lavoro", "").expect("space");
    let notebook = a
        .database
        .create_notebook(Some(&space.id), "Progetti", "")
        .expect("notebook");
    let note = a
        .database
        .create_note_in_notebook("Verbale", &doc("decisioni sul budget"), Some(&notebook.id))
        .expect("note");
    let tag = a.database.create_tag("riunione", None).expect("tag");
    a.database
        .set_note_tags(&note.id, std::slice::from_ref(&tag.id))
        .expect("note tag");
    let task = a
        .database
        .create_task(&TaskInput {
            title: "Inviare il verbale".to_owned(),
            description: String::new(),
            priority: 1,
            due_date: Some("2026-10-22".to_owned()),
            note_id: Some(note.id.clone()),
            flagged: false,
            recurrence: None,
        })
        .expect("task");
    let event = a
        .database
        .create_event(&EventInput {
            title: "Revisione".to_owned(),
            description: String::new(),
            all_day: true,
            start: "2026-10-23".to_owned(),
            end: "2026-10-23".to_owned(),
            timezone: Some("Europe/Rome".to_owned()),
            note_id: Some(note.id.clone()),
            recurrence: None,
        })
        .expect("event");
    let now = "2026-10-20T10:00:00Z".parse().expect("now");
    a.database
        .create_reminder(
            ReminderEntity::Task,
            &task.id,
            "2026-10-22T09:00",
            Some("Europe/Rome"),
            now,
        )
        .expect("reminder");

    let first = sync(&a);
    assert!(first.exported >= 8, "{first:?}");
    let received = sync(&b);
    assert_eq!(received.imported, first.exported);
    assert_eq!(received.rejected, 0);
    assert!(received.changed_notes.contains(&note.id));

    // Tutto arrivato, con gli stessi id, e la ricerca funziona.
    let copied = b.database.get_note(&note.id).expect("note on b");
    assert_eq!(copied.notebook_id.as_deref(), Some(notebook.id.as_str()));
    assert_eq!(
        b.database
            .search_notes("budget", "all", None, 10, 0)
            .expect("search")
            .len(),
        1
    );
    assert_eq!(
        b.database.list_note_tags(&note.id).expect("tags")[0].name,
        "riunione"
    );
    assert_eq!(
        b.database.get_task(&task.id).expect("task").title,
        "Inviare il verbale"
    );
    assert_eq!(
        b.database.get_event(&event.id).expect("event").title,
        "Revisione"
    );
    assert_eq!(b.database.list_reminders().expect("reminders").len(), 1);
    assert_eq!(b.database.list_spaces().expect("spaces")[0].name, "Lavoro");

    // Nessuna modifica: niente da esportare né importare (idempotente).
    assert_eq!(sync(&b).exported, 0);
    let again = sync(&a);
    assert_eq!((again.exported, again.imported), (0, 0));

    // Modifica su B, ritorno su A.
    b.database
        .update_note(&note.id, "Verbale definitivo", &doc("decisioni sul budget"))
        .expect("edit");
    b.database
        .set_task_completed(
            &task.id,
            true,
            crate::time::now(),
            &crate::time::Zone::system(),
        )
        .expect("done");
    sync(&b);
    let back = sync(&a);
    // Nota, attività e il promemoria chiuso dal completamento.
    assert!(back.imported >= 2, "{back:?}");
    assert_eq!(
        a.database.get_note(&note.id).expect("note").title,
        "Verbale definitivo"
    );
    assert_eq!(
        a.database.get_task(&task.id).expect("task").status,
        crate::domain::planner::TaskStatus::Done
    );
    // Le modifiche applicate non rimbalzano.
    assert_eq!(sync(&a).exported, 0);

    let status = a.database.sync_status().expect("status");
    assert_eq!(status.devices.len(), 2);
    assert!(
        status
            .devices
            .iter()
            .any(|device| device.name == "Fisso" && !device.this_device)
    );
    assert_eq!(status.pending_changes, 0);
    assert!(status.last_error.is_none());
}

#[test]
fn concurrent_note_edits_converge_and_keep_the_losing_text() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let note = a.database.create_note("Lista", &doc("pane")).expect("note");
    sync(&a);
    sync(&b);

    a.database
        .update_note(&note.id, "Lista", &doc("pane, latte"))
        .expect("a edit");
    b.database
        .update_note(&note.id, "Lista", &doc("pane, uova"))
        .expect("b edit");
    sync(&a);
    let on_b = sync(&b);
    let on_a = sync(&a);
    assert_eq!(
        on_b.conflicts + on_a.conflicts,
        2,
        "ognuno conserva la versione dell'altro"
    );

    // Stesso risultato su entrambi.
    assert_eq!(note_text(&a, &note.id), note_text(&b, &note.id));
    // E nessun testo perso: il perdente è nella cronologia della nota.
    for device in [&a, &b] {
        let revisions = device
            .database
            .list_note_revisions(&note.id)
            .expect("revisions");
        let mut texts: Vec<String> = revisions
            .iter()
            .map(|revision| {
                let detail = device
                    .database
                    .get_note_revision(&revision.id)
                    .expect("detail");
                crate::content::to_plain_text(&detail.content)
            })
            .collect();
        texts.push(note_text(device, &note.id));
        let all = texts.join(" | ");
        assert!(all.contains("latte") && all.contains("uova"), "{all}");
        let conflicts = device.database.sync_status().expect("status").conflicts;
        assert_eq!(conflicts.len(), 1);
        assert!(
            device
                .database
                .dismiss_sync_conflict(&conflicts[0].id)
                .expect("dismiss")
        );
    }
    // Convergenza stabile: un altro giro non cambia nulla.
    assert_eq!(sync(&b).imported, 0);
}

#[test]
fn a_device_joining_later_does_not_report_old_conflicts_between_others() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let note = a.database.create_note("Lista", &doc("pane")).expect("note");
    sync(&a);
    sync(&b);
    // Modifiche contemporanee tra A e B: ognuno registra il conflitto.
    a.database
        .update_note(&note.id, "Lista", &doc("pane, latte"))
        .expect("a edit");
    b.database
        .update_note(&note.id, "Lista", &doc("pane, uova"))
        .expect("b edit");
    sync(&a);
    sync(&b);
    sync(&a);
    assert_eq!(a.database.sync_status().expect("status").conflicts.len(), 1);

    // C entra dopo, con una nota propria: riceve la storia di A e B.
    let c = device(shared.path(), "C");
    let own = c
        .database
        .create_note("Solo su C", &doc("mia"))
        .expect("own");
    let first = sync(&c);
    assert_eq!(
        first.conflicts, 0,
        "la storia degli altri non è un conflitto di C"
    );
    assert!(
        c.database
            .sync_status()
            .expect("status")
            .conflicts
            .is_empty()
    );
    assert_eq!(note_text(&c, &note.id), note_text(&a, &note.id));
    assert_eq!(note_text(&c, &own.id), "Solo su C: mia");

    // Dopo l'ingresso i conflitti veri di C vengono registrati come sempre.
    sync(&a);
    a.database
        .update_note(&own.id, "Solo su C", &doc("modificata su A"))
        .expect("a edits c note");
    c.database
        .update_note(&own.id, "Solo su C", &doc("modificata su C"))
        .expect("c edits own note");
    sync(&a);
    let later = sync(&c);
    assert_eq!(
        later.conflicts, 1,
        "un conflitto che riguarda C resta visibile"
    );
}

#[test]
fn edits_win_over_concurrent_permanent_deletion() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let note = a.database.create_note("Idee", &doc("prima")).expect("note");
    sync(&a);
    sync(&b);

    a.database.trash_note(&note.id).expect("trash");
    a.database
        .delete_note_permanently(&note.id, a.database.data_directory().expect("dir"))
        .expect("purge");
    b.database
        .update_note(&note.id, "Idee", &doc("seconda, importante"))
        .expect("edit");
    sync(&a);
    sync(&b);
    sync(&a);
    for device in [&a, &b] {
        assert_eq!(note_text(device, &note.id), "Idee: seconda, importante");
    }

    // Eliminazione senza modifiche contemporanee: arriva a tutti.
    b.database.trash_note(&note.id).expect("trash");
    b.database
        .delete_note_permanently(&note.id, b.database.data_directory().expect("dir"))
        .expect("purge");
    sync(&b);
    sync(&a);
    assert!(a.database.get_note(&note.id).is_err());
    assert!(
        a.database
            .search_notes("importante", "all", None, 10, 0)
            .expect("search")
            .is_empty()
    );
}

#[test]
fn batches_wait_for_their_dependencies() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let notebook = a
        .database
        .create_notebook(None, "Viaggi", "")
        .expect("notebook");
    sync(&a);
    sync(&b);
    let note = b
        .database
        .create_note_in_notebook("Valigia", &doc("costume"), Some(&notebook.id))
        .expect("note");
    sync(&b);

    // Su C il lotto di A non è ancora arrivato (servizio lento).
    let a_device = a
        .database
        .sync_status()
        .expect("status")
        .device_id
        .expect("id");
    let changes = shared
        .path()
        .join(folder::ROOT)
        .join("devices")
        .join(&a_device)
        .join("changes");
    let hidden = shared.path().join("in-transito");
    fs::rename(&changes, &hidden).expect("hide");
    let c = device(shared.path(), "C");
    let report = sync(&c);
    assert!(report.waiting);
    assert!(
        c.database.get_note(&note.id).is_err(),
        "applicato prima delle dipendenze"
    );

    fs::rename(&hidden, &changes).expect("restore");
    let report = sync(&c);
    assert!(!report.waiting);
    assert_eq!(
        c.database
            .get_note(&note.id)
            .expect("note")
            .notebook_id
            .as_deref(),
        Some(notebook.id.as_str())
    );
}

#[test]
fn tags_with_the_same_name_are_merged() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let on_a = a.database.create_note("Nota A", &doc("a")).expect("note");
    let on_b = b.database.create_note("Nota B", &doc("b")).expect("note");
    let tag_a = a.database.create_tag("Lavoro", None).expect("tag");
    let tag_b = b.database.create_tag("lavoro", None).expect("tag");
    a.database
        .set_note_tags(&on_a.id, std::slice::from_ref(&tag_a.id))
        .expect("tag a");
    b.database
        .set_note_tags(&on_b.id, std::slice::from_ref(&tag_b.id))
        .expect("tag b");
    sync(&a);
    sync(&b);
    sync(&a);
    for device in [&a, &b] {
        let tags = device.database.list_tags().expect("tags");
        assert_eq!(tags.len(), 1, "{tags:?}");
        for note in [&on_a, &on_b] {
            let names: Vec<String> = device
                .database
                .list_note_tags(&note.id)
                .expect("tags")
                .into_iter()
                .map(|tag| tag.name)
                .collect();
            assert_eq!(names.len(), 1);
            assert!(names[0].eq_ignore_ascii_case("lavoro"));
        }
    }
    assert_eq!(titles(&a), titles(&b));
}

#[test]
fn attachments_travel_as_verified_blobs() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let note = a
        .database
        .create_note("Con allegato", &doc("vedi file"))
        .expect("note");
    let data_a = a.database.data_directory().expect("dir").to_path_buf();
    let attachment = a
        .database
        .add_attachment(&note.id, "appunti.txt", b"contenuto riservato", &data_a)
        .expect("attachment");
    sync(&a);

    // Il blob non è ancora arrivato su B: allegato in attesa, non perso.
    let blob_dir = shared
        .path()
        .join(folder::ROOT)
        .join("blobs")
        .join(&attachment.sha256[..2]);
    let parked = shared.path().join("blob-in-transito");
    fs::rename(&blob_dir, &parked).expect("hide blob");
    let report = sync(&b);
    assert_eq!(report.rejected, 0, "{report:?}");
    assert_eq!(
        b.database.list_attachments(&note.id).expect("list").len(),
        1
    );
    assert_eq!(
        b.database
            .sync_status()
            .expect("status")
            .missing_attachments,
        1
    );

    fs::rename(&parked, &blob_dir).expect("restore blob");
    sync(&b);
    assert_eq!(
        b.database
            .sync_status()
            .expect("status")
            .missing_attachments,
        0
    );
    let data_b = b.database.data_directory().expect("dir").to_path_buf();
    let path = b
        .database
        .verified_attachment_path(&note.id, &attachment.id, &data_b)
        .expect("path");
    assert_eq!(fs::read(path).expect("read"), b"contenuto riservato");

    // Rimozione dell'allegato: sparisce anche il file sull'altro dispositivo.
    b.database
        .delete_attachment(&note.id, &attachment.id, &data_b)
        .expect("delete");
    sync(&b);
    sync(&a);
    assert!(
        a.database
            .list_attachments(&note.id)
            .expect("list")
            .is_empty()
    );
    let leftovers = fs::read_dir(data_a.join("attachments"))
        .expect("dir")
        .count();
    assert_eq!(leftovers, 0, "file dell'allegato rimasto");
}

#[test]
fn copied_database_is_detected_and_external_events_stay_local() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    a.database.create_note("Prima", &doc("x")).expect("note");
    sync(&a);

    // Lo stesso database copiato su un altro computer.
    let copy_dir = tempfile::tempdir().expect("copy");
    let source = a.database.path().expect("path").to_path_buf();
    fs::copy(&source, copy_dir.path().join("gna.sqlite3")).expect("copy");
    let clone = Database::open(copy_dir.path().join("gna.sqlite3")).expect("open copy");
    a.database.create_note("Seconda", &doc("y")).expect("note");
    sync(&a);
    clone.create_note("Terza", &doc("z")).expect("note");
    assert!(matches!(clone.sync_now(), Err(SyncError::DuplicateDevice)));
    assert!(clone.sync_status().expect("status").last_error.is_some());

    // Riattivata con una nuova identità, si unisce senza perdere note.
    let settings = clone.sync_settings().expect("settings");
    clone
        .set_sync_settings(SyncSettings {
            enabled: false,
            ..settings.clone()
        })
        .expect("off");
    clone.set_sync_settings(settings).expect("on");
    clone.sync_now().expect("sync");
    sync(&a);
    let mut names: Vec<String> = a
        .database
        .search_notes("", "all", None, 10, 0)
        .expect("notes")
        .into_iter()
        .map(|r| r.note.title)
        .collect();
    names.sort();
    assert_eq!(names, ["Prima", "Seconda", "Terza"]);

    // Gli abbonamenti iCal e i loro eventi non vengono sincronizzati.
    let subscription = a
        .database
        .add_calendar_subscription("Festività", None)
        .expect("sub");
    crate::calendar_sync::subscriptions::apply_ics(
        &a.database,
        &subscription,
        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nSUMMARY:Festa\r\nDTSTART;VALUE=DATE:20261101\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        "2026-10-20T10:00:00Z".parse().expect("now"),
        &jiff::tz::TimeZone::UTC,
    )
    .expect("ics");
    assert_eq!(sync(&a).exported, 0);
}

#[test]
fn restoring_an_older_backup_stops_sync_without_losing_data() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    a.database.create_note("Prima", &doc("x")).expect("note");
    sync(&a);
    let backup = a
        .database
        .create_backup(crate::backup::BackupKind::Manual)
        .expect("backup");
    a.database.create_note("Seconda", &doc("y")).expect("note");
    sync(&a);

    // Il backup non conosce l'ultimo lotto che A ha già scritto nella
    // cartella: riprendere con la stessa identità riutilizzerebbe i suoi
    // numeri di sequenza.
    a.database
        .restore_backup(&backup.file_name)
        .expect("restore");
    a.database.create_note("Terza", &doc("z")).expect("note");
    assert!(matches!(
        a.database.sync_now(),
        Err(SyncError::DuplicateDevice)
    ));
    sync(&b);
    assert_eq!(titles(&b), ["Prima", "Seconda"]);

    // Con una nuova identità A si ricongiunge: nessuna nota va persa.
    let settings = a.database.sync_settings().expect("settings");
    a.database
        .set_sync_settings(SyncSettings {
            enabled: false,
            ..settings.clone()
        })
        .expect("off");
    a.database.set_sync_settings(settings).expect("on");
    sync(&a);
    sync(&b);
    assert_eq!(titles(&a), ["Prima", "Seconda", "Terza"]);
    assert_eq!(titles(&b), ["Prima", "Seconda", "Terza"]);
}

#[test]
fn editor_save_keeps_a_remote_edit_it_did_not_see() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let note = a
        .database
        .create_note("Piano", &doc("versione iniziale"))
        .expect("note");
    sync(&a);
    sync(&b);
    // L'editor di B ha aperto la nota a questa versione.
    let opened = b.database.get_note(&note.id).expect("note");

    a.database
        .update_note(&note.id, "Piano", &doc("modifica dal portatile"))
        .expect("edit");
    sync(&a);
    sync(&b);
    // L'editor di B salva il proprio testo senza aver visto la modifica.
    b.database
        .update_note_from(
            &note.id,
            "Piano",
            &doc("modifica dal fisso"),
            Some(&opened.updated_at),
        )
        .expect("save");
    let status = b.database.sync_status().expect("status");
    assert_eq!(status.conflicts.len(), 1);
    let revisions = b.database.list_note_revisions(&note.id).expect("revisions");
    assert!(
        revisions
            .iter()
            .any(|revision| revision.preview.contains("modifica dal portatile"))
    );

    // Un editor già aggiornato salva senza conflitti.
    let current = b.database.get_note(&note.id).expect("note");
    b.database
        .update_note_from(&note.id, "Piano", &doc("ancora"), Some(&current.updated_at))
        .expect("save");
    assert_eq!(b.database.sync_status().expect("status").conflicts.len(), 1);

    // Prima della sincronizzazione nulla viene registrato.
    assert!(matches!(
        Database::in_memory().expect("db").sync_now(),
        Err(SyncError::NotConfigured)
    ));
}

#[test]
fn rejected_operations_leave_no_partial_state() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let note = a.database.create_note("Nota", &doc("x")).expect("note");
    sync(&a);
    sync(&b);
    // Lotto ostile scritto a mano: nome di file con un percorso.
    let a_id = a
        .database
        .sync_status()
        .expect("status")
        .device_id
        .expect("id");
    let status = b.database.sync_status().expect("status");
    let applied = status
        .devices
        .iter()
        .find(|device| device.device_id == a_id)
        .expect("a")
        .applied_seq as u64;
    let library =
        fs::read_to_string(shared.path().join(folder::ROOT).join("format.json")).expect("format");
    let library: serde_json::Value = serde_json::from_str(&library).expect("json");
    let batch = folder::Batch {
        format: folder::FORMAT,
        library: library["library"].as_str().expect("library").to_owned(),
        device: a_id.clone(),
        seq: applied + 1,
        deps: Default::default(),
        created_at: "2026-10-20T10:00:00Z".to_owned(),
        ops: vec![folder::Operation {
            entity: "attachments".to_owned(),
            key: uuid::Uuid::new_v4().to_string(),
            version: folder::Version(1000, a_id.clone()),
            base: None,
            data: Some(json!({
                "note_id": note.id, "original_filename": "x.png", "stored_filename": "../../evil.png",
                "mime_type": "image/png", "size_bytes": 10, "sha256": "0".repeat(64), "created_at": "2026-10-20T10:00:00Z",
                "deleted_at": null, "search_text": "", "ocr_text": null, "ocr_status": null, "ocr_engine": null,
                "ocr_error": null, "ocr_at": null
            })),
        }],
    };
    let path = shared
        .path()
        .join(folder::ROOT)
        .join("devices")
        .join(&a_id)
        .join("changes")
        .join(format!("{:012}.json", applied + 1));
    fs::write(&path, serde_json::to_vec(&batch).expect("json")).expect("write");
    let report = sync(&b);
    assert_eq!(report.rejected, 1);
    assert!(
        b.database
            .list_attachments(&note.id)
            .expect("list")
            .is_empty(),
        "riga parziale rimasta"
    );
}

/// Archivio remoto in memoria (come Dropbox: nessun file parziale).
#[derive(Clone, Default)]
struct MemoryStore(std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>>);

impl folder::SyncStore for MemoryStore {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, FolderError> {
        Ok(self.0.lock().expect("lock").get(path).cloned())
    }
    fn write(&self, path: &str, bytes: &[u8], create_only: bool) -> Result<(), FolderError> {
        let mut files = self.0.lock().expect("lock");
        if create_only && files.contains_key(path) {
            return Err(FolderError::Exists);
        }
        files.insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }
    fn exists(&self, path: &str) -> Result<bool, FolderError> {
        Ok(self.0.lock().expect("lock").contains_key(path))
    }
    fn list(&self, directory: &str) -> Result<Vec<(String, bool)>, FolderError> {
        let prefix = format!("{directory}/");
        let mut items: Vec<(String, bool)> = Vec::new();
        for path in self.0.lock().expect("lock").keys() {
            if let Some(rest) = path.strip_prefix(&prefix) {
                let (name, is_dir) = match rest.split_once('/') {
                    Some((name, _)) => (name.to_owned(), true),
                    None => (rest.to_owned(), false),
                };
                if !items.iter().any(|(existing, _)| *existing == name) {
                    items.push((name, is_dir));
                }
            }
        }
        Ok(items)
    }
    fn remove(&self, path: &str) -> Result<(), FolderError> {
        let prefix = format!("{path}/");
        self.0
            .lock()
            .expect("lock")
            .retain(|key, _| key != path && !key.starts_with(&prefix));
        Ok(())
    }
}

fn remote_device(store: &MemoryStore, name: &str) -> Device {
    let directory = tempfile::tempdir().expect("dir");
    let database = Database::open(directory.path().join("gna.sqlite3")).expect("open");
    database
        .set_sync_settings(SyncSettings {
            enabled: true,
            transport: SyncTransport::Dropbox,
            folder: String::new(),
            dropbox_account: "anna@example.com".to_owned(),
            device_name: name.to_owned(),
            encrypted: false,
        })
        .expect("settings");
    let _ = store;
    Device {
        _directory: directory,
        database,
    }
}

#[test]
fn remote_store_carries_notes_and_attachments() {
    let store = MemoryStore::default();
    let a = remote_device(&store, "A");
    let b = remote_device(&store, "B");
    // Senza archivio fornito, la sincronizzazione Dropbox non parte.
    assert!(matches!(
        a.database.sync_now(),
        Err(SyncError::NotConfigured)
    ));

    let note = a
        .database
        .create_note("Da Dropbox", &doc("ciao"))
        .expect("note");
    let data_a = a.database.data_directory().expect("dir").to_path_buf();
    let attachment = a
        .database
        .add_attachment(&note.id, "file.txt", b"dati riservati", &data_a)
        .expect("att");
    a.database
        .sync_now_with(
            Some(Box::new(store.clone())),
            &mut folder::SyncAccess::default(),
        )
        .expect("sync a");
    let report = b
        .database
        .sync_now_with(
            Some(Box::new(store.clone())),
            &mut folder::SyncAccess::default(),
        )
        .expect("sync b");
    assert_eq!(report.rejected, 0);
    assert_eq!(
        b.database.get_note(&note.id).expect("note").title,
        "Da Dropbox"
    );
    let data_b = b.database.data_directory().expect("dir").to_path_buf();
    let path = b
        .database
        .verified_attachment_path(&note.id, &attachment.id, &data_b)
        .expect("path");
    assert_eq!(fs::read(path).expect("read"), b"dati riservati");
    assert!(
        store
            .0
            .lock()
            .expect("lock")
            .keys()
            .any(|key| key == "format.json")
    );

    // Passare a un'altra destinazione azzera la storia locale del protocollo.
    let mut settings = b.database.sync_settings().expect("settings");
    settings.dropbox_account = "altro@example.com".to_owned();
    b.database.set_sync_settings(settings).expect("settings");
    assert!(
        b.database
            .sync_status()
            .expect("status")
            .device_id
            .is_none()
    );
}

/// Archivio che verifica che nessuna operazione di rete avvenga con il
/// database bloccato (Dropbox può essere lento).
struct WatchingStore {
    inner: MemoryStore,
    database: std::sync::Arc<Database>,
    calls_while_locked: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl WatchingStore {
    fn check(&self) {
        if !self.database.connection_is_free() {
            self.calls_while_locked
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

impl folder::SyncStore for WatchingStore {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, FolderError> {
        self.check();
        self.inner.read(path)
    }
    fn write(&self, path: &str, bytes: &[u8], create_only: bool) -> Result<(), FolderError> {
        self.check();
        self.inner.write(path, bytes, create_only)
    }
    fn exists(&self, path: &str) -> Result<bool, FolderError> {
        self.check();
        self.inner.exists(path)
    }
    fn list(&self, directory: &str) -> Result<Vec<(String, bool)>, FolderError> {
        self.check();
        self.inner.list(directory)
    }
    fn remove(&self, path: &str) -> Result<(), FolderError> {
        self.check();
        self.inner.remove(path)
    }
}

#[test]
fn network_calls_never_hold_the_database_lock() {
    let store = MemoryStore::default();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut devices = Vec::new();
    for name in ["A", "B"] {
        let device = remote_device(&store, name);
        let note = device
            .database
            .create_note(name, &doc("testo"))
            .expect("note");
        let data = device.database.data_directory().expect("dir").to_path_buf();
        device
            .database
            .add_attachment(&note.id, "a.txt", name.as_bytes(), &data)
            .expect("att");
        let Device {
            _directory,
            database,
        } = device;
        devices.push((_directory, std::sync::Arc::new(database)));
    }
    for _ in 0..2 {
        for (_, database) in &devices {
            let watching = WatchingStore {
                inner: store.clone(),
                database: std::sync::Arc::clone(database),
                calls_while_locked: std::sync::Arc::clone(&calls),
            };
            database
                .sync_now_with(Some(Box::new(watching)), &mut folder::SyncAccess::default())
                .expect("sync");
        }
    }
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "rete con il database bloccato"
    );
    for (_, database) in &devices {
        assert_eq!(
            database
                .search_notes("", "all", None, 10, 0)
                .expect("notes")
                .len(),
            2
        );
    }
}

fn device_names(device: &Device) -> Vec<String> {
    let mut names: Vec<String> = device
        .database
        .sync_status()
        .expect("status")
        .devices
        .into_iter()
        .map(|device| device.name)
        .collect();
    names.sort();
    names
}

fn device_id_named(device: &Device, name: &str) -> String {
    device
        .database
        .sync_status()
        .expect("status")
        .devices
        .into_iter()
        .find(|device| device.name == name)
        .map(|device| device.device_id)
        .expect("device")
}

#[test]
fn removing_a_device_from_the_list_hides_it_everywhere_and_keeps_its_data() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let c = device(shared.path(), "C");
    // Un dispositivo compare nell'elenco degli altri dopo il primo invio.
    a.database
        .create_note("Da A", &doc("scritta su A"))
        .expect("note");
    b.database
        .create_note("Da B", &doc("scritta su B"))
        .expect("note");
    c.database
        .create_note("Da C", &doc("scritta su C"))
        .expect("note");
    sync(&b);
    sync(&a);
    sync(&c);
    sync(&a);
    assert_eq!(device_names(&a), ["A", "B", "C"]);

    let b_id = device_id_named(&a, "B");
    a.database.retire_sync_device(&b_id).expect("retire");
    assert_eq!(device_names(&a), ["A", "C"], "subito nascosto su A");
    sync(&a);
    sync(&c);
    assert_eq!(device_names(&c), ["A", "C"], "nascosto anche su C");

    // Chi entra dopo riceve comunque le note di B, senza vederlo in elenco.
    let d = device(shared.path(), "D");
    sync(&d);
    assert_eq!(titles(&d), ["Da A", "Da B", "Da C"]);
    assert_eq!(device_names(&d), ["A", "C", "D"]);

    // B torna a sincronizzare, anche nello stesso secondo: ricompare per
    // tutti. (D non è nell'elenco di A finché non invia una modifica.)
    sync(&b);
    sync(&a);
    assert_eq!(device_names(&a), ["A", "B", "C"]);
    sync(&c);
    assert!(device_names(&c).contains(&"B".to_owned()));
}

#[test]
fn this_device_cannot_be_removed_from_the_list() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    sync(&a);
    let own = a
        .database
        .sync_status()
        .expect("status")
        .device_id
        .expect("id");
    assert!(a.database.retire_sync_device(&own).is_err());
    assert!(
        a.database
            .retire_sync_device(&Uuid::new_v4().to_string())
            .is_err()
    );
}

#[test]
fn default_device_name_is_never_empty() {
    let name = default_device_name();
    assert!(!name.trim().is_empty());
    assert!(name.chars().count() <= 100);
}

// ---------------------------------------------------------------------------
// Cifratura (ADR 0032)

const PASSPHRASE: &str = "cavallo batteria graffetta";

fn set_encrypted(device: &Device) {
    let settings = device.database.sync_settings().expect("settings");
    device
        .database
        .set_sync_settings(SyncSettings {
            encrypted: true,
            ..settings
        })
        .expect("settings");
}

fn sync_with(device: &Device, access: &mut folder::SyncAccess) -> Result<SyncReport, SyncError> {
    device.database.sync_now_with(None, access)
}

fn unlock(passphrase: &str) -> folder::SyncAccess {
    folder::SyncAccess {
        passphrase: Some(passphrase.to_owned()),
        ..Default::default()
    }
}

/// Tutti i file della libreria (percorso relativo, contenuto).
fn library_files(shared: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, directory: &Path, files: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(directory).expect("dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("relative")
                    .display()
                    .to_string();
                files.push((relative, fs::read(&path).expect("read")));
            }
        }
    }
    let root = shared.join(folder::ROOT);
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    files
}

fn contains(bytes: &[u8], needle: &str) -> bool {
    bytes
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[test]
fn encrypted_library_hides_contents_and_needs_the_passphrase() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "Portatile di Anna");
    set_encrypted(&a);
    let note = a
        .database
        .create_note("Diagnosi riservata", &doc("pressione alta"))
        .expect("note");
    let data_a = a.database.data_directory().expect("dir").to_path_buf();
    let attachment = a
        .database
        .add_attachment(&note.id, "referto.txt", b"valori del referto", &data_a)
        .expect("attachment");

    // Senza passphrase, o troppo corta, la libreria non viene creata.
    assert!(matches!(
        sync_with(&a, &mut folder::SyncAccess::default()),
        Err(SyncError::Folder(FolderError::PassphraseRequired))
    ));
    assert!(
        a.database
            .sync_status()
            .expect("status")
            .passphrase_required
    );
    assert!(matches!(
        sync_with(&a, &mut unlock("breve")),
        Err(SyncError::Folder(FolderError::Crypto(_)))
    ));
    let mut access_a = unlock(PASSPHRASE);
    sync_with(&a, &mut access_a).expect("sync");
    let key = access_a.key.clone().expect("chiave da conservare");
    let status = a.database.sync_status().expect("status");
    assert!(status.encrypted && !status.passphrase_required);

    // Nella cartella niente testo, nomi, passphrase né hash degli allegati.
    let files = library_files(shared.path());
    assert!(files.iter().any(|(path, _)| path == "format.json"));
    for (path, bytes) in &files {
        for secret in [
            "Diagnosi",
            "pressione",
            "Anna",
            "referto",
            "valori del referto",
            PASSPHRASE,
            attachment.sha256.as_str(),
            key.to_hex().as_str(),
        ] {
            assert!(
                !contains(bytes, secret) && !path.contains(secret),
                "{path}: {secret}"
            );
        }
        if path != "format.json" && path != "encrypted/format.json" {
            assert!(path.starts_with("encrypted/"), "{path}");
        }
    }
    let root: Value = serde_json::from_slice(
        &fs::read(shared.path().join(folder::ROOT).join("format.json")).expect("read"),
    )
    .expect("json");
    assert_eq!(root["format"], folder::ENCRYPTED_FORMAT);

    // Un altro dispositivo: senza passphrase si ferma, con quella sbagliata
    // pure, con quella giusta riceve nota e allegato.
    let b = device(shared.path(), "Telefono");
    assert!(matches!(
        sync_with(&b, &mut folder::SyncAccess::default()),
        Err(SyncError::Folder(FolderError::PassphraseRequired))
    ));
    assert!(titles(&b).is_empty());
    assert!(matches!(
        sync_with(&b, &mut unlock("passphrase sbagliata")),
        Err(SyncError::Folder(FolderError::Crypto(
            crypto::CryptoError::WrongPassphrase
        )))
    ));
    let mut access_b = unlock(PASSPHRASE);
    sync_with(&b, &mut access_b).expect("sync");
    assert_eq!(titles(&b), vec!["Diagnosi riservata"]);
    let data_b = b.database.data_directory().expect("dir").to_path_buf();
    let path = b
        .database
        .verified_attachment_path(&note.id, &attachment.id, &data_b)
        .expect("allegato decifrato e verificato");
    assert_eq!(fs::read(path).expect("read"), b"valori del referto");

    // Le volte successive basta la chiave salvata.
    b.database
        .update_note(&note.id, "Diagnosi riservata", &doc("pressione normale"))
        .expect("update");
    let mut saved = folder::SyncAccess {
        key: access_b.key.clone(),
        ..Default::default()
    };
    sync_with(&b, &mut saved).expect("sync");
    sync_with(
        &a,
        &mut folder::SyncAccess {
            key: Some(key),
            ..Default::default()
        },
    )
    .expect("sync");
    assert!(note_text(&a, &note.id).contains("normale"));

    // Un lotto alterato non viene applicato.
    let batch = files
        .iter()
        .find(|(path, _)| path.contains("/changes/"))
        .map(|(path, _)| shared.path().join(folder::ROOT).join(path))
        .expect("batch");
    let mut bytes = fs::read(&batch).expect("read");
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x55;
    fs::write(&batch, bytes).expect("tamper");
    let c = device(shared.path(), "Nuovo");
    let report = sync_with(&c, &mut unlock(PASSPHRASE)).expect("sync");
    assert!(!report.warnings.is_empty(), "{report:?}");
}

#[test]
fn plain_library_is_converted_without_losing_changes() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    a.database.create_note("Da A", &doc("uno")).expect("note");
    sync(&a);
    sync(&b);
    // B scrive una nota che non arriva mai nella libreria in chiaro.
    b.database
        .create_note("Solo su B", &doc("due"))
        .expect("note");

    set_encrypted(&a);
    a.database
        .create_note("Dopo la cifratura", &doc("tre"))
        .expect("note");
    let mut access_a = unlock(PASSPHRASE);
    sync_with(&a, &mut access_a).expect("conversion");
    assert!(a.database.sync_status().expect("status").encrypted);
    // La libreria in chiaro resta (da cancellare a mano), ma è ferma.
    assert!(shared.path().join(folder::ROOT).join("devices").is_dir());

    // B si accorge della conversione e chiede la passphrase.
    assert!(matches!(
        sync_with(&b, &mut folder::SyncAccess::default()),
        Err(SyncError::Folder(FolderError::PassphraseRequired))
    ));
    sync_with(&b, &mut unlock(PASSPHRASE)).expect("join");
    assert!(b.database.sync_status().expect("status").encrypted);
    sync_with(&a, &mut access_a).expect("sync");
    let expected = vec!["Da A", "Dopo la cifratura", "Solo su B"];
    assert_eq!(titles(&a), expected);
    assert_eq!(titles(&b), expected);
    // Nessuna nota duplicata né conflitto per le note già uguali.
    assert!(
        a.database
            .sync_status()
            .expect("status")
            .conflicts
            .is_empty()
    );

    // Una versione di Greatnote senza cifratura vede un formato più nuovo
    // e si ferma invece di scrivere in chiaro.
    let root: Value = serde_json::from_slice(
        &fs::read(shared.path().join(folder::ROOT).join("format.json")).expect("read"),
    )
    .expect("json");
    assert!(root["format"].as_u64().expect("format") > u64::from(folder::FORMAT));
}

#[test]
fn passphrase_change_keeps_the_key_for_other_devices() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    set_encrypted(&a);
    a.database.create_note("Prima", &doc("x")).expect("note");
    let mut access_a = unlock(PASSPHRASE);
    sync_with(&a, &mut access_a).expect("sync");
    let mut access_b = unlock(PASSPHRASE);
    sync_with(&b, &mut access_b).expect("sync");

    let mut with_key = folder::SyncAccess {
        key: access_a.key.clone(),
        ..Default::default()
    };
    assert!(
        a.database
            .change_sync_passphrase(None, &mut with_key, "corta")
            .is_err()
    );
    a.database
        .change_sync_passphrase(None, &mut with_key, "una passphrase nuova e lunga")
        .expect("change");

    // B continua con la chiave salvata.
    a.database.create_note("Dopo", &doc("y")).expect("note");
    sync_with(&a, &mut with_key).expect("sync");
    let mut saved_b = folder::SyncAccess {
        key: access_b.key.clone(),
        ..Default::default()
    };
    sync_with(&b, &mut saved_b).expect("sync");
    assert_eq!(titles(&b), vec!["Dopo", "Prima"]);

    // Un nuovo dispositivo deve usare la passphrase nuova.
    let c = device(shared.path(), "C");
    assert!(sync_with(&c, &mut unlock(PASSPHRASE)).is_err());
    sync_with(&c, &mut unlock("una passphrase nuova e lunga")).expect("sync");
    assert_eq!(titles(&c), vec!["Dopo", "Prima"]);
}

// ---------------------------------------------------------------------------
// Istantanee per chi entra (ADR 0033)

/// Stato confrontabile di un dispositivo: note con testo, tag e allegati.
fn library_state(device: &Device) -> Vec<String> {
    let mut state: Vec<String> = device
        .database
        .search_notes("", "all", None, 500, 0)
        .expect("notes")
        .into_iter()
        .map(|result| {
            let id = result.note.id;
            let mut tags: Vec<String> = device
                .database
                .list_note_tags(&id)
                .expect("tags")
                .into_iter()
                .map(|tag| tag.name)
                .collect();
            tags.sort();
            let attachments: Vec<String> = device
                .database
                .list_attachments(&id)
                .expect("attachments")
                .into_iter()
                .map(|attachment| attachment.sha256)
                .collect();
            format!(
                "{} [{}] {:?}",
                note_text(device, &id),
                tags.join(","),
                attachments
            )
        })
        .collect();
    state.sort();
    state
}

fn snapshot_manifests(shared: &Path, device_id: &str, encrypted: bool) -> Vec<std::path::PathBuf> {
    let mut path = shared.join(folder::ROOT);
    if encrypted {
        path = path.join("encrypted");
    }
    let directory = path.join("devices").join(device_id).join("snapshots");
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect()
}

fn device_id(device: &Device) -> String {
    device
        .database
        .sync_status()
        .expect("status")
        .device_id
        .expect("device id")
}

fn snapshot_scenario(encrypted: bool) {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let mut access_a = folder::SyncAccess::default();
    let mut access_b = folder::SyncAccess::default();
    if encrypted {
        set_encrypted(&a);
        access_a.passphrase = Some(PASSPHRASE.to_owned());
        access_b.passphrase = Some(PASSPHRASE.to_owned());
    }
    let run = |device: &Device, access: &mut folder::SyncAccess| {
        let report = sync_with(device, access).expect("sync");
        assert_eq!(report.rejected, 0, "{report:?}");
        report
    };

    // Storia lunga: molte sincronizzazioni, ognuna con un lotto.
    let notebook = a
        .database
        .create_notebook(None, "Lavoro", "")
        .expect("notebook");
    let tag = a.database.create_tag("urgente", None).expect("tag");
    let kept = a
        .database
        .create_note_in_notebook("Verbale", &doc("prima versione"), Some(&notebook.id))
        .expect("note");
    a.database
        .set_note_tags(&kept.id, std::slice::from_ref(&tag.id))
        .expect("tags");
    let data_a = a.database.data_directory().expect("dir").to_path_buf();
    let attachment = a
        .database
        .add_attachment(&kept.id, "foto.txt", b"contenuto dell'allegato", &data_a)
        .expect("attachment");
    let gone = a
        .database
        .create_note("Da eliminare", &doc("x"))
        .expect("note");
    run(&a, &mut access_a);
    run(&b, &mut access_b);
    for round in 0..4 {
        a.database
            .update_note(&kept.id, "Verbale", &doc(&format!("versione {round}")))
            .expect("edit");
        run(&a, &mut access_a);
        b.database
            .create_note(&format!("Nota di B {round}"), &doc("b"))
            .expect("note");
        run(&b, &mut access_b);
    }
    a.database.trash_note(&gone.id).expect("trash");
    a.database
        .delete_note_permanently(&gone.id, &data_a)
        .expect("delete");
    run(&a, &mut access_a);
    run(&b, &mut access_b);
    run(&a, &mut access_a);

    let (id_a, id_b) = (device_id(&a), device_id(&b));
    let written = snapshot_manifests(shared.path(), &id_a, encrypted).len()
        + snapshot_manifests(shared.path(), &id_b, encrypted).len();
    assert!(written > 0, "nessuna istantanea scritta");
    // Al massimo due per dispositivo.
    assert!(snapshot_manifests(shared.path(), &id_a, encrypted).len() <= 2);

    // Nella libreria in chiaro si tolgono i lotti coperti: il nuovo
    // dispositivo può contare solo sull'istantanea.
    if !encrypted {
        let mut best: Option<Value> = None;
        for path in snapshot_manifests(shared.path(), &id_a, false)
            .into_iter()
            .chain(snapshot_manifests(shared.path(), &id_b, false))
        {
            let manifest: Value =
                serde_json::from_slice(&fs::read(path).expect("read")).expect("json");
            let total = |m: &Value| -> u64 {
                m["covers"]
                    .as_object()
                    .expect("covers")
                    .values()
                    .filter_map(Value::as_u64)
                    .sum()
            };
            if best
                .as_ref()
                .is_none_or(|current| total(&manifest) > total(current))
            {
                best = Some(manifest);
            }
        }
        let best = best.expect("manifest");
        let mut removed = 0;
        for (device, covered) in best["covers"].as_object().expect("covers") {
            let covered = covered.as_u64().expect("seq");
            let changes = shared
                .path()
                .join(folder::ROOT)
                .join("devices")
                .join(device)
                .join("changes");
            for seq in 1..=covered {
                if fs::remove_file(changes.join(format!("{seq:012}.json"))).is_ok() {
                    removed += 1;
                }
            }
        }
        assert!(removed >= 5, "lotti coperti tolti: {removed}");
    }

    // Un nuovo dispositivo arriva allo stesso stato, nota eliminata esclusa.
    let c = device(shared.path(), "C");
    let mut access_c = folder::SyncAccess::default();
    if encrypted {
        access_c.passphrase = Some(PASSPHRASE.to_owned());
    }
    run(&c, &mut access_c);
    run(&c, &mut access_c);
    assert_eq!(library_state(&c), library_state(&a));
    assert!(c.database.get_note(&gone.id).is_err());
    assert!(
        library_state(&c)
            .iter()
            .any(|line| line.contains("versione 3") && line.contains("urgente"))
    );
    let data_c = c.database.data_directory().expect("dir").to_path_buf();
    let path = c
        .database
        .verified_attachment_path(&kept.id, &attachment.id, &data_c)
        .expect("allegato dall'istantanea");
    assert_eq!(fs::read(path).expect("read"), b"contenuto dell'allegato");
    assert!(
        c.database
            .sync_status()
            .expect("status")
            .conflicts
            .is_empty()
    );
    // Gli altri dispositivi compaiono con il loro nome.
    let names: Vec<String> = c
        .database
        .sync_status()
        .expect("status")
        .devices
        .into_iter()
        .map(|device| device.name)
        .collect();
    assert!(
        names.contains(&"A".to_owned()) && names.contains(&"B".to_owned()),
        "{names:?}"
    );

    // Dopo l'ingresso tutto continua come prima, conflitti compresi.
    b.database
        .create_note("Dopo l'istantanea", &doc("nuova"))
        .expect("note");
    run(&b, &mut access_b);
    c.database
        .update_note(&kept.id, "Verbale", &doc("versione di C"))
        .expect("edit");
    a.database
        .update_note(&kept.id, "Verbale", &doc("versione di A"))
        .expect("edit");
    run(&c, &mut access_c);
    run(&a, &mut access_a);
    run(&b, &mut access_b);
    run(&c, &mut access_c);
    run(&a, &mut access_a);
    assert_eq!(library_state(&a), library_state(&b));
    assert_eq!(library_state(&a), library_state(&c));
    assert!(titles(&c).contains(&"Dopo l'istantanea".to_owned()));
    let conflicts = a.database.sync_status().expect("status").conflicts.len()
        + c.database.sync_status().expect("status").conflicts.len();
    assert_eq!(conflicts, 2, "ognuno conserva la versione dell'altro");

    if encrypted {
        for (path, bytes) in library_files(shared.path()) {
            assert!(
                !contains(&bytes, "Verbale") && !contains(&bytes, "urgente"),
                "{path}"
            );
        }
    }
}

#[test]
fn new_devices_start_from_a_snapshot() {
    snapshot_scenario(false);
}

#[test]
fn snapshots_of_an_encrypted_library_are_encrypted() {
    snapshot_scenario(true);
}

#[test]
fn a_damaged_snapshot_falls_back_to_the_batches() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    for round in 0..5 {
        a.database
            .create_note(&format!("Nota {round}"), &doc("testo"))
            .expect("note");
        sync(&a);
    }
    sync(&a);
    let manifests = snapshot_manifests(shared.path(), &device_id(&a), false);
    assert!(!manifests.is_empty());
    // Parti dell'istantanea danneggiate: si rileggono i lotti.
    for manifest in &manifests {
        let parts = manifest.with_extension("");
        for entry in fs::read_dir(parts).expect("parts") {
            fs::write(entry.expect("entry").path(), b"{rotto").expect("damage");
        }
    }
    let b = device(shared.path(), "B");
    let report = sync(&b);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("istantanea")),
        "{report:?}"
    );
    assert_eq!(titles(&b), titles(&a));
}

// ---------------------------------------------------------------------------
// Fusione di modifiche contemporanee (ADR 0034)

fn paragraphs(texts: &[&str]) -> Value {
    json!({"type": "doc", "content": texts.iter().map(|text| json!({
        "type": "paragraph", "content": [{"type": "text", "text": text}]
    })).collect::<Vec<_>>()})
}

fn revision_texts(device: &Device, note_id: &str) -> Vec<String> {
    device
        .database
        .list_note_revisions(note_id)
        .expect("revisions")
        .iter()
        .map(|revision| {
            let detail = device
                .database
                .get_note_revision(&revision.id)
                .expect("detail");
            crate::content::to_plain_text(&detail.content)
        })
        .collect()
}

#[test]
fn edits_to_different_paragraphs_are_merged_on_every_device() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let note = a
        .database
        .create_note(
            "Verbale",
            &paragraphs(&["Presenti: Anna", "Decisioni", "Prossimi passi"]),
        )
        .expect("note");
    sync(&a);
    sync(&b);

    a.database
        .update_note(
            &note.id,
            "Verbale",
            &paragraphs(&["Presenti: Anna, Bruno", "Decisioni", "Prossimi passi"]),
        )
        .expect("a");
    b.database
        .update_note(
            &note.id,
            "Verbale del lunedì",
            &paragraphs(&[
                "Presenti: Anna",
                "Decisioni",
                "Prossimi passi",
                "Chiamare il fornitore",
            ]),
        )
        .expect("b");
    let mut merged = 0;
    for _ in 0..3 {
        merged += sync(&a).merged;
        merged += sync(&b).merged;
    }
    assert!(merged >= 1);
    let expected = "Verbale del lunedì: Presenti: Anna, Bruno\n\nDecisioni\n\nProssimi passi\n\nChiamare il fornitore";
    assert_eq!(note_text(&a, &note.id), expected);
    assert_eq!(note_text(&b, &note.id), expected);
    for device in [&a, &b] {
        // Nessun conflitto da controllare; il testo di prima è nella cronologia.
        assert!(
            device
                .database
                .sync_status()
                .expect("status")
                .conflicts
                .is_empty()
        );
    }
    let history = [revision_texts(&a, &note.id), revision_texts(&b, &note.id)]
        .concat()
        .join(" | ");
    assert!(
        history.contains("Anna, Bruno") || history.contains("fornitore"),
        "{history}"
    );
    // Convergenza stabile.
    sync(&a);
    sync(&b);
    assert_eq!(sync(&a).imported + sync(&b).imported, 0);
    // Un dispositivo che arriva dopo vede lo stesso testo.
    let c = device(shared.path(), "C");
    sync(&c);
    sync(&c);
    assert_eq!(note_text(&c, &note.id), expected);
    assert!(
        c.database
            .sync_status()
            .expect("status")
            .conflicts
            .is_empty()
    );

    // Stesso paragrafo cambiato su due dispositivi: nessuna fusione, resta
    // la regola di sempre con il conflitto da controllare.
    a.database
        .update_note(
            &note.id,
            "Verbale del lunedì",
            &paragraphs(&[
                "Presenti: tutti",
                "Decisioni",
                "Prossimi passi",
                "Chiamare il fornitore",
            ]),
        )
        .expect("a");
    c.database
        .update_note(
            &note.id,
            "Verbale del lunedì",
            &paragraphs(&[
                "Presenti: nessuno",
                "Decisioni",
                "Prossimi passi",
                "Chiamare il fornitore",
            ]),
        )
        .expect("c");
    let mut reports = Vec::new();
    for _ in 0..2 {
        reports.push(sync(&a));
        reports.push(sync(&c));
        reports.push(sync(&b));
    }
    assert_eq!(reports.iter().map(|report| report.merged).sum::<usize>(), 0);
    assert!(reports.iter().map(|report| report.conflicts).sum::<usize>() >= 2);
    assert_eq!(note_text(&a, &note.id), note_text(&c, &note.id));
    assert_eq!(note_text(&a, &note.id), note_text(&b, &note.id));
}

#[test]
fn three_devices_editing_different_paragraphs_converge() {
    let shared = tempfile::tempdir().expect("shared");
    let devices: Vec<Device> = ["A", "B", "C"]
        .iter()
        .map(|name| device(shared.path(), name))
        .collect();
    let note = devices[0]
        .database
        .create_note("Lista", &paragraphs(&["uno", "due", "tre", "quattro"]))
        .expect("note");
    for device in &devices {
        sync(device);
    }
    for device in &devices {
        sync(device);
    }
    let edits = [
        ["UNO", "due", "tre", "quattro"],
        ["uno", "due", "TRE", "quattro"],
        ["uno", "due", "tre", "quattro"],
    ];
    for (device, texts) in devices.iter().zip(edits) {
        let mut texts: Vec<&str> = texts.to_vec();
        if device
            .database
            .sync_status()
            .expect("status")
            .settings
            .device_name
            == "C"
        {
            texts.push("cinque");
        }
        device
            .database
            .update_note(&note.id, "Lista", &paragraphs(&texts))
            .expect("edit");
    }
    for _ in 0..4 {
        for device in &devices {
            sync(device);
        }
    }
    let expected = "Lista: UNO\n\ndue\n\nTRE\n\nquattro\n\ncinque";
    for device in &devices {
        assert_eq!(note_text(device, &note.id), expected);
    }
    // Fermi: nessuna altra modifica in giro.
    let imported: usize = devices.iter().map(|device| sync(device).imported).sum();
    assert_eq!(imported, 0);
}

#[test]
fn without_a_known_base_the_usual_rule_applies() {
    let shared = tempfile::tempdir().expect("shared");
    let a = device(shared.path(), "A");
    let b = device(shared.path(), "B");
    let note = a
        .database
        .create_note("Nota", &paragraphs(&["uno", "due"]))
        .expect("note");
    sync(&a);
    sync(&b);
    // Come dopo l'aggiornamento da una versione senza basi conservate.
    for device in [&a, &b] {
        device
            .database
            .connection()
            .expect("connection")
            .execute("DELETE FROM sync_note_bases", [])
            .expect("clear");
    }
    a.database
        .update_note(&note.id, "Nota", &paragraphs(&["UNO", "due"]))
        .expect("a");
    b.database
        .update_note(&note.id, "Nota", &paragraphs(&["uno", "DUE"]))
        .expect("b");
    let mut merged = 0;
    let mut conflicts = 0;
    for _ in 0..2 {
        for device in [&a, &b] {
            let report = sync(device);
            merged += report.merged;
            conflicts += report.conflicts;
        }
    }
    assert_eq!(merged, 0);
    assert_eq!(conflicts, 2);
    assert_eq!(note_text(&a, &note.id), note_text(&b, &note.id));
}

#[test]
fn random_concurrent_edits_always_converge() {
    let shared = tempfile::tempdir().expect("shared");
    let devices: Vec<Device> = ["A", "B", "C"]
        .iter()
        .map(|name| device(shared.path(), name))
        .collect();
    let note = devices[0]
        .database
        .create_note("Diario", &paragraphs(&["p0", "p1", "p2", "p3", "p4", "p5"]))
        .expect("note");
    for _ in 0..2 {
        for device in &devices {
            sync(device);
        }
    }
    let mut seed: u64 = std::env::var("GREATNOTE_MERGE_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(7);
    let mut next = |limit: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % limit
    };
    for round in 0..25 {
        for (index, device) in devices.iter().enumerate() {
            if next(2) == 0 {
                continue;
            }
            let current = device.database.get_note(&note.id).expect("note");
            let mut blocks: Vec<String> = current.content["content"]
                .as_array()
                .expect("blocks")
                .iter()
                .map(|block| {
                    crate::content::to_plain_text(&json!({"type": "doc", "content": [block]}))
                })
                .collect();
            let position = next(blocks.len() as u64 + 1) as usize;
            match next(3) {
                0 if position < blocks.len() => {
                    blocks[position] = format!("{} r{round}d{index}", blocks[position])
                }
                1 if position < blocks.len() && blocks.len() > 2 => {
                    blocks.remove(position);
                }
                _ => blocks.insert(position, format!("nuovo r{round}d{index}")),
            }
            let texts: Vec<&str> = blocks.iter().map(String::as_str).collect();
            device
                .database
                .update_note(&note.id, "Diario", &paragraphs(&texts))
                .expect("edit");
        }
        let order = next(6) as usize;
        for step in 0..3 {
            sync(&devices[(order + step) % 3]);
        }
    }
    for _ in 0..5 {
        for device in &devices {
            sync(device);
        }
    }
    let first = note_text(&devices[0], &note.id);
    for device in &devices[1..] {
        assert_eq!(note_text(device, &note.id), first);
    }
    let imported: usize = devices.iter().map(|device| sync(device).imported).sum();
    assert_eq!(imported, 0, "ancora modifiche in giro");
}
