BEGIN IMMEDIATE;

-- La ricostruzione v3→v4 escludeva le note nel cestino: ripristinarle le
-- lasciava fuori dalla ricerca testuale. Reindicizza ogni nota mancante.
INSERT INTO note_search (note_id, title, content, attachments)
SELECT notes.id, notes.title,
       COALESCE((
           SELECT group_concat(node.value, ' ')
           FROM json_tree(notes.content_json) AS node
           WHERE node.type = 'text'
       ), ''),
       COALESCE((
           SELECT group_concat(attachments.search_text, ' ')
           FROM attachments
           WHERE attachments.note_id = notes.id
             AND attachments.deleted_at IS NULL
       ), '')
FROM notes
WHERE NOT EXISTS (
    SELECT 1 FROM note_search WHERE note_search.note_id = notes.id
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL CHECK (length(key) BETWEEN 1 AND 100),
    value TEXT NOT NULL CHECK (length(value) <= 4096),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Provenienza facoltativa di una nota importata (es. "markdown", "html",
-- "gna-archive"); non contiene percorsi locali dell'utente.
ALTER TABLE notes ADD COLUMN source TEXT CHECK (source IS NULL OR length(source) <= 100);

PRAGMA user_version = 5;
COMMIT;
