BEGIN IMMEDIATE;

-- Cronologia locale delle note: istantanee della versione precedente,
-- registrate al massimo ogni 10 minuti durante la modifica e prima di ogni
-- ripristino. Spariscono con la nota (eliminazione definitiva).
CREATE TABLE note_revisions (
    id TEXT PRIMARY KEY NOT NULL,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (length(title) <= 512),
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX note_revisions_note ON note_revisions (note_id, created_at DESC);

-- Modelli di nota creati dall'utente; passano dal cestino come gli altri
-- elementi.
CREATE TABLE templates (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 200),
    title TEXT NOT NULL DEFAULT '' CHECK (length(title) <= 512),
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    deleted_at TEXT
);

-- Attività contrassegnate (bandierina), per evidenziare quelle importanti.
ALTER TABLE tasks ADD COLUMN flagged INTEGER NOT NULL DEFAULT 0 CHECK (flagged IN (0, 1));

PRAGMA user_version = 8;
COMMIT;
