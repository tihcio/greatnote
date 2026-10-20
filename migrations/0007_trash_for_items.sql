BEGIN IMMEDIATE;

-- Cestino per spazi, taccuini, tag, attività ed eventi. Spazi e taccuini
-- avevano già `deleted_at` (finora inutilizzato).
ALTER TABLE tags ADD COLUMN deleted_at TEXT;
ALTER TABLE tasks ADD COLUMN deleted_at TEXT;
ALTER TABLE events ADD COLUMN deleted_at TEXT;

-- Collegamenti staccati quando un contenitore va nel cestino, per
-- ricrearli al ripristino: note di un taccuino, taccuini di uno spazio, note
-- di un tag. Così il resto dell'app non vede mai riferimenti a elementi nel
-- cestino. Le righe spariscono al ripristino o all'eliminazione definitiva.
CREATE TABLE trash_relations (
    item_type TEXT NOT NULL CHECK (item_type IN ('space', 'notebook', 'tag')),
    item_id TEXT NOT NULL,
    related_id TEXT NOT NULL,
    PRIMARY KEY (item_type, item_id, related_id)
);

CREATE INDEX spaces_deleted_at ON spaces (deleted_at);
CREATE INDEX notebooks_deleted_at ON notebooks (deleted_at);
CREATE INDEX tags_deleted_at ON tags (deleted_at);
CREATE INDEX tasks_deleted_at ON tasks (deleted_at);
CREATE INDEX events_deleted_at ON events (deleted_at);

PRAGMA user_version = 7;
COMMIT;
