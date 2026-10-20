BEGIN IMMEDIATE;

-- Indice semantico locale (Fase 5). Per ogni nota indicizzata si conservano
-- il modello di embedding e la data di modifica della versione indicizzata:
-- se la nota cambia o cambia il modello, va reindicizzata e nel frattempo
-- non viene usata. L'impronta di titolo e testo evita di ricalcolare gli
-- embedding quando cambiano solo i metadati (fissata, spostata, ...).
-- I frammenti contengono solo posizioni nel testo e vettori, non copie del
-- testo: il contenuto inviato all'AI viene sempre riletto dalla nota
-- corrente. Tutto sparisce con la nota.
CREATE TABLE ai_index_notes (
    note_id TEXT PRIMARY KEY NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    model TEXT NOT NULL CHECK (length(model) BETWEEN 1 AND 200),
    note_updated_at TEXT NOT NULL,
    source_hash TEXT NOT NULL CHECK (length(source_hash) = 64),
    indexed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE ai_chunks (
    note_id TEXT NOT NULL REFERENCES ai_index_notes(note_id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
    char_start INTEGER NOT NULL CHECK (char_start >= 0),
    char_end INTEGER NOT NULL CHECK (char_end > char_start),
    dimensions INTEGER NOT NULL CHECK (dimensions BETWEEN 1 AND 8192),
    vector BLOB NOT NULL CHECK (length(vector) = dimensions * 4),
    PRIMARY KEY (note_id, chunk_index)
);

PRAGMA user_version = 9;
COMMIT;
