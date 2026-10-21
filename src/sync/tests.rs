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
    b.database.set_task_completed(&task.id, true).expect("done");
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
        .sync_now_with(Some(Box::new(store.clone())))
        .expect("sync a");
    let report = b
        .database
        .sync_now_with(Some(Box::new(store.clone())))
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
