//! Prova della sincronizzazione su una COPIA di una cartella dati reale:
//! esporta tutto su un dispositivo, importa su un dispositivo nuovo e
//! confronta conteggi e allegati. Ignorata di default; non usare mai la
//! cartella dati originale.
//!
//! ```text
//! GREATNOTE_SYNC_SOURCE_DIR=/percorso/copia-dati \
//!   cargo test --release --test sync_live -- --ignored --nocapture
//! ```

use std::{fs, path::Path, time::Instant};

use greatnote_core::{storage::Database, sync::SyncSettings};

fn count(database: &Database, scope: &str) -> usize {
    let mut total = 0;
    loop {
        let page = database
            .search_notes("", scope, None, 200, total as u32)
            .expect("notes");
        total += page.len();
        if page.len() < 200 {
            return total;
        }
    }
}

fn folder_size(path: &Path) -> u64 {
    fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| {
                    let path = entry.path();
                    if path.is_dir() {
                        folder_size(&path)
                    } else {
                        entry.metadata().map(|m| m.len()).unwrap_or(0)
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

#[test]
#[ignore = "richiede una copia di una cartella dati reale"]
fn real_library_round_trip() {
    let source = std::env::var("GREATNOTE_SYNC_SOURCE_DIR").expect("GREATNOTE_SYNC_SOURCE_DIR");
    let shared = tempfile::tempdir().expect("shared");
    let settings = |name: &str| SyncSettings {
        enabled: true,
        transport: Default::default(),
        dropbox_account: String::new(),
        folder: shared.path().display().to_string(),
        device_name: name.to_owned(),
    };

    let started = Instant::now();
    let a = Database::open(Path::new(&source).join("gna.sqlite3")).expect("open copy");
    println!("apertura e migrazione: {:?}", started.elapsed());
    a.set_sync_settings(settings("Copia A")).expect("settings");
    let started = Instant::now();
    let exported = a.sync_now().expect("sync a");
    println!(
        "esportazione: {} operazioni in {:?}, cartella {} MB",
        exported.exported,
        started.elapsed(),
        folder_size(shared.path()) / 1_000_000
    );

    let b_dir = tempfile::tempdir().expect("b");
    let b = Database::open(b_dir.path().join("gna.sqlite3")).expect("open b");
    b.set_sync_settings(settings("Nuovo B")).expect("settings");
    let started = Instant::now();
    let imported = b.sync_now().expect("sync b");
    println!(
        "importazione: {} operazioni, {} scartate, {} conflitti in {:?}",
        imported.imported,
        imported.rejected,
        imported.conflicts,
        started.elapsed()
    );
    assert_eq!(imported.rejected, 0);
    assert_eq!(imported.conflicts, 0);

    for scope in ["all", "archived", "trash"] {
        assert_eq!(count(&a, scope), count(&b, scope), "{scope}");
        println!("note «{scope}»: {}", count(&b, scope));
    }
    assert_eq!(
        a.list_tags().expect("tags").len(),
        b.list_tags().expect("tags").len()
    );
    assert_eq!(
        a.list_notebooks().expect("nb").len(),
        b.list_notebooks().expect("nb").len()
    );
    let status = b.sync_status().expect("status");
    assert_eq!(status.missing_attachments, 0);
    // Allegati identici byte per byte.
    let b_attachments = b_dir.path().join("attachments");
    let mut files = 0;
    if let Ok(entries) = fs::read_dir(&b_attachments) {
        for entry in entries.filter_map(Result::ok) {
            let original = fs::read(
                Path::new(&source)
                    .join("attachments")
                    .join(entry.file_name()),
            )
            .expect("original");
            assert_eq!(fs::read(entry.path()).expect("copy"), original);
            files += 1;
        }
    }
    println!("allegati copiati e verificati: {files}");

    // Secondo giro senza modifiche: niente da fare, e veloce.
    let started = Instant::now();
    let idle = a.sync_now().expect("idle");
    assert_eq!((idle.exported, idle.imported), (0, 0));
    println!("giro senza modifiche: {:?}", started.elapsed());
}
