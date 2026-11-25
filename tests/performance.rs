//! Prove di prestazioni ripetibili su un archivio sintetico (decine di
//! migliaia di note, con taccuini, tag, attività ed eventi ricorrenti).
//! Ignorata di default perché lenta: crea i dati in una cartella temporanea,
//! mai nella cartella dati dell'utente.
//!
//! ```text
//! cargo test --release --test performance -- --ignored --nocapture
//! GREATNOTE_PERF_NOTES=50000 cargo test --release --test performance -- --ignored --nocapture
//! ```
//!
//! Le soglie sono larghe: servono a cogliere peggioramenti di un ordine di
//! grandezza, non a misurare la macchina. I tempi vengono stampati.

use std::time::{Duration, Instant};

use greatnote_core::{
    backup::BackupKind,
    domain::planner::{EventInput, TaskInput, TaskView},
    storage::Database,
    time::Zone,
};
use serde_json::{Value, json};

const WORDS: &[&str] = &[
    "progetto",
    "riunione",
    "budget",
    "cliente",
    "fattura",
    "piano",
    "verbale",
    "obiettivo",
    "scadenza",
    "fornitore",
    "contratto",
    "relazione",
    "bozza",
    "idea",
    "ricetta",
    "viaggio",
    "libro",
    "allenamento",
    "spesa",
    "medico",
    "scuola",
    "casa",
    "giardino",
    "auto",
    "banca",
    "assicurazione",
    "manutenzione",
    "regalo",
    "compleanno",
    "vacanza",
    "lavoro",
    "corso",
    "lezione",
    "appunti",
    "elenco",
    "domanda",
    "risposta",
    "prova",
    "analisi",
    "sintesi",
];

/// Generatore deterministico (stesso archivio a ogni esecuzione).
struct Words(u64);

impl Words {
    fn next(&mut self) -> &'static str {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        WORDS[((self.0 >> 33) % WORDS.len() as u64) as usize]
    }

    fn sentence(&mut self, count: usize) -> String {
        (0..count)
            .map(|_| self.next())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn document(text: &str) -> Value {
    json!({ "type": "doc", "content": [
        { "type": "paragraph", "content": [{ "type": "text", "text": text }] },
        { "type": "bulletList", "content": [
            { "type": "listItem", "content": [{ "type": "paragraph",
                "content": [{ "type": "text", "text": "primo punto" }] }] }
        ]}
    ]})
}

/// Mediana di più esecuzioni (la prima scalda la cache).
fn measure<T>(label: &str, limit: Duration, mut task: impl FnMut() -> T) -> Duration {
    task();
    let mut times: Vec<Duration> = (0..5)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(task());
            start.elapsed()
        })
        .collect();
    times.sort();
    let median = times[times.len() / 2];
    println!("{label:<48} {:>9.2} ms", median.as_secs_f64() * 1000.0);
    assert!(
        median <= limit,
        "{label}: {median:?} oltre la soglia {limit:?}"
    );
    median
}

#[test]
#[ignore = "lenta: cargo test --release --test performance -- --ignored --nocapture"]
fn large_archive_stays_responsive() {
    let notes: usize = std::env::var("GREATNOTE_PERF_NOTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(30_000);
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("gna.sqlite3");
    let database = Database::open(&path).expect("database");

    let start = Instant::now();
    let notebooks: Vec<String> = (0..40)
        .map(|index| {
            database
                .create_notebook(None, &format!("Taccuino {index}"), "")
                .expect("notebook")
                .id
        })
        .collect();
    let tags: Vec<String> = (0..60)
        .map(|index| {
            database
                .create_tag(&format!("tag{index}"), None)
                .expect("tag")
                .id
        })
        .collect();
    let mut words = Words(42);
    let mut note_ids = Vec::with_capacity(notes);
    for index in 0..notes {
        let title = format!("{} {index}", words.sentence(3));
        let body = words.sentence(80);
        let note = database
            .create_note_in_notebook(
                &title,
                &document(&body),
                Some(&notebooks[index % notebooks.len()]),
            )
            .expect("note");
        if index % 3 == 0 {
            database
                .set_note_tags(
                    &note.id,
                    &[
                        tags[index % tags.len()].clone(),
                        tags[(index * 7) % tags.len()].clone(),
                    ],
                )
                .expect("tags");
        }
        note_ids.push(note.id);
    }
    for index in 0..2_000 {
        database
            .create_task(&TaskInput {
                title: format!("Attività {} {index}", words.sentence(2)),
                description: words.sentence(10),
                priority: (index % 3) as u8,
                due_date: Some(format!("2026-{:02}-{:02}", index % 12 + 1, index % 28 + 1)),
                note_id: Some(note_ids[index % note_ids.len()].clone()),
                flagged: index % 10 == 0,
                recurrence: None,
            })
            .expect("task");
    }
    for index in 0..300 {
        database
            .create_event(&EventInput {
                title: format!("Evento {index}"),
                description: String::new(),
                all_day: false,
                start: format!("2026-{:02}-{:02}T09:00", index % 12 + 1, index % 28 + 1),
                end: format!("2026-{:02}-{:02}T10:00", index % 12 + 1, index % 28 + 1),
                timezone: Some("Europe/Rome".to_owned()),
                note_id: None,
                recurrence: (index % 5 == 0)
                    .then(|| serde_json::from_str(r#"{"frequency":"weekly"}"#).expect("rule")),
            })
            .expect("event");
    }
    println!(
        "\narchivio sintetico: {notes} note, 2000 attività, 300 eventi creati in {:.1} s\n",
        start.elapsed().as_secs_f64()
    );
    drop(database);

    let ms = Duration::from_millis;
    measure("apertura del database (avvio)", ms(1500), || {
        Database::open(&path).expect("reopen")
    });
    let database = Database::open(&path).expect("reopen");
    measure("contatori della barra laterale", ms(300), || {
        database.sidebar_counts().expect("counts")
    });
    measure("elenco note: prima pagina (50)", ms(150), || {
        database.search_notes("", "all", None, 50, 0).expect("page")
    });
    measure(
        "elenco note: pagina profonda (offset 20000)",
        ms(800),
        || {
            database
                .search_notes("", "all", None, 50, 20_000)
                .expect("page")
        },
    );
    measure("elenco di un taccuino", ms(200), || {
        database
            .search_notes("", "notebook", Some(&notebooks[3]), 50, 0)
            .expect("notebook")
    });
    measure("ricerca: una parola comune", ms(400), || {
        database
            .search_notes("progetto", "all", None, 50, 0)
            .expect("search")
    });
    measure(
        "ricerca: prefisso mentre si scrive (\"fat\")",
        ms(400),
        || {
            database
                .search_notes("fat", "all", None, 50, 0)
                .expect("search")
        },
    );
    measure("ricerca: frase esatta", ms(400), || {
        database
            .search_notes("\"budget cliente\"", "all", None, 50, 0)
            .expect("search")
    });
    measure("ricerca: parola rara", ms(200), || {
        database
            .search_notes("inesistente", "all", None, 50, 0)
            .expect("search")
    });
    measure("ricerca: con filtri (tag e taccuino)", ms(400), || {
        database
            .search_notes(
                "riunione tag:tag5 notebook:\"Taccuino 5\"",
                "all",
                None,
                50,
                0,
            )
            .expect("search")
    });
    measure("ricerca: operatori (A OR B) NOT C", ms(800), || {
        database
            .search_notes("(fattura OR contratto) NOT bozza", "all", None, 50, 0)
            .expect("search")
    });
    measure("ricerca: OR tra testo e filtro", ms(800), || {
        database
            .search_notes("viaggio OR tag:tag7", "all", None, 50, 0)
            .expect("search")
    });
    let today = greatnote_core::time::parse_date("2026-06-15").expect("date");
    measure("attività: aperte con ricerca", ms(300), || {
        database
            .list_tasks(TaskView::Open, today, Some("riunione"))
            .expect("tasks")
    });
    let rome = Zone::stored("Europe/Rome");
    let (from, to) = (
        greatnote_core::time::parse_date("2026-06-01").expect("date"),
        greatnote_core::time::parse_date("2026-07-12").expect("date"),
    );
    measure("calendario: mese con eventi ricorrenti", ms(300), || {
        database.list_events(from, to, &rome).expect("events")
    });
    measure("lettura di una nota", ms(50), || {
        database.get_note(&note_ids[notes / 2]).expect("note")
    });
    let mut edit = 0;
    measure("salvataggio di una nota (testo cambiato)", ms(200), || {
        // Testo diverso a ogni giro: un salvataggio identico viene saltato.
        edit += 1;
        database
            .update_note(
                &note_ids[notes / 3],
                "Nota modificata",
                &document(&format!("testo aggiornato {edit}")),
            )
            .expect("update")
    });
    let integrity = Instant::now();
    let report = database.check_integrity().expect("integrity");
    println!(
        "{:<48} {:>9.2} ms",
        "controllo d'integrità (una volta)",
        integrity.elapsed().as_secs_f64() * 1000.0
    );
    assert!(report.ok, "integrità: {report:?}");
    let backup = Instant::now();
    database.create_backup(BackupKind::Manual).expect("backup");
    let backup_time = backup.elapsed();
    println!(
        "{:<48} {:>9.2} ms",
        "backup completo (una volta)",
        backup_time.as_secs_f64() * 1000.0
    );
    assert!(
        backup_time < Duration::from_secs(30),
        "backup lento: {backup_time:?}"
    );
}
