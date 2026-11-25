//! Contenuto ricevuto da altre app (pannello «Condividi» di Android):
//! diventa una nuova nota, con il testo nel corpo e i file come allegati.

use std::path::Path;

use serde::Serialize;

use crate::{
    content::text_to_document,
    domain::note::Note,
    storage::{Database, MAX_ATTACHMENT_BYTES, StorageError, sniff_attachment_type},
};

/// File condivisi accettati al massimo in una volta.
pub const MAX_SHARED_FILES: usize = 20;
/// Testo condiviso conservato al massimo (caratteri).
const MAX_SHARED_TEXT: usize = 200_000;
const MAX_TITLE_CHARS: usize = 120;

/// File ricevuto, già letto.
pub struct SharedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SharedNote {
    pub note: Note,
    /// File non allegati (tipo non supportato, troppo grande…), con il motivo.
    pub skipped: Vec<String>,
}

/// Titolo della nota: l'oggetto indicato dall'altra app, altrimenti la prima
/// riga del testo, altrimenti il nome del primo file.
fn shared_title(subject: Option<&str>, text: &str, files: &[SharedFile]) -> String {
    let candidate = subject
        .map(str::trim)
        .filter(|subject| !subject.is_empty())
        .or_else(|| text.lines().map(str::trim).find(|line| !line.is_empty()))
        .or_else(|| files.first().map(|file| file.name.as_str()))
        .unwrap_or_default();
    let mut title: String = candidate.chars().take(MAX_TITLE_CHARS).collect();
    if candidate.chars().count() > MAX_TITLE_CHARS {
        title.push('…');
    }
    if title.is_empty() {
        crate::tr!("Condiviso da un'altra app", "Shared from another app")
    } else {
        title
    }
}

impl Database {
    /// Crea una nota con il contenuto condiviso. I file di tipo non
    /// supportato o troppo grandi vengono saltati e indicati in `skipped`;
    /// se non resta niente (né testo né allegati) la nota non viene creata.
    pub fn create_note_from_share(
        &self,
        subject: Option<&str>,
        text: Option<&str>,
        files: &[SharedFile],
        data_directory: &Path,
    ) -> Result<SharedNote, StorageError> {
        let text: String = text
            .unwrap_or_default()
            .chars()
            .take(MAX_SHARED_TEXT)
            .collect();
        let title = shared_title(subject, &text, files);
        if text.trim().is_empty() && files.is_empty() {
            return Err(StorageError::InvalidImport(crate::tr!(
                "Il contenuto condiviso è vuoto",
                "The shared content is empty"
            )));
        }
        let document = if text.trim().is_empty() {
            crate::domain::note::empty_document()
        } else {
            text_to_document(&text)
        };
        // Tipo e dimensione si controllano prima di creare la nota: se non
        // resta niente da salvare, nessuna nota vuota.
        let mut skipped = Vec::new();
        let mut accepted = Vec::new();
        for (index, file) in files.iter().enumerate() {
            let problem = if index >= MAX_SHARED_FILES {
                Some(crate::tr!(
                    "troppi file (al massimo {})",
                    "too many files (at most {})",
                    MAX_SHARED_FILES
                ))
            } else if file.bytes.len() > MAX_ATTACHMENT_BYTES {
                Some(crate::tr!(
                    "file troppo grande (al massimo 25 MB)",
                    "file too large (at most 25 MB)"
                ))
            } else {
                sniff_attachment_type(&file.bytes)
                    .err()
                    .map(|error| error.to_string())
            };
            match problem {
                Some(problem) => skipped.push(format!("{}: {problem}", file.name)),
                None => accepted.push(file),
            }
        }
        if accepted.is_empty() && text.trim().is_empty() {
            return Err(StorageError::InvalidImport(skipped.join("; ")));
        }
        let note = self.create_note(&title, &document)?;
        for file in accepted {
            if let Err(error) =
                self.add_attachment(&note.id, &file.name, &file.bytes, data_directory)
            {
                skipped.push(format!("{}: {error}", file.name));
            }
        }
        Ok(SharedNote {
            note: self.get_note(&note.id)?,
            skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        // PNG 1×1 valido.
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 0x0A, 0x49,
            0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0D, 0x0A, 0x2D, 0xB4, 0, 0,
            0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    #[test]
    fn shared_text_and_files_become_a_note() {
        let directory = tempfile::tempdir().expect("directory");
        let database = Database::open(directory.path().join("gna.sqlite3")).expect("database");
        let shared = database
            .create_note_from_share(
                None,
                Some("Articolo interessante\nhttps://example.com/pagina\n\nDa leggere"),
                &[
                    SharedFile {
                        name: "foto.png".to_owned(),
                        bytes: png(),
                    },
                    SharedFile {
                        name: "programma.exe".to_owned(),
                        bytes: b"MZ\x90\x00".to_vec(),
                    },
                ],
                directory.path(),
            )
            .expect("note");
        assert_eq!(shared.note.title, "Articolo interessante");
        assert!(
            shared
                .note
                .content
                .to_string()
                .contains("https://example.com/pagina")
        );
        let attachments = database
            .list_attachments(&shared.note.id)
            .expect("attachments");
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].original_filename, "foto.png");
        assert_eq!(shared.skipped.len(), 1);
        assert!(shared.skipped[0].starts_with("programma.exe"));
    }

    #[test]
    fn subject_wins_and_nothing_usable_creates_no_note() {
        let directory = tempfile::tempdir().expect("directory");
        let database = Database::open(directory.path().join("gna.sqlite3")).expect("database");
        let shared = database
            .create_note_from_share(
                Some("  Ricetta  "),
                Some("farina, uova"),
                &[],
                directory.path(),
            )
            .expect("note");
        assert_eq!(shared.note.title, "Ricetta");
        // Solo un file non accettato: errore e nessuna nota rimasta.
        let error = database.create_note_from_share(
            None,
            None,
            &[SharedFile {
                name: "x.bin".to_owned(),
                bytes: vec![0, 1, 2],
            }],
            directory.path(),
        );
        assert!(error.is_err());
        assert!(
            database
                .create_note_from_share(None, Some("   "), &[], directory.path())
                .is_err()
        );
        assert_eq!(database.list_notes(10, 0).expect("notes").len(), 1);
        // Titolo lungo accorciato.
        let long = "parola ".repeat(40);
        let shared = database
            .create_note_from_share(None, Some(&long), &[], directory.path())
            .expect("note");
        assert!(shared.note.title.chars().count() <= MAX_TITLE_CHARS + 1);
        assert!(shared.note.title.ends_with('…'));
    }
}
