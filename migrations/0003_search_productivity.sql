BEGIN IMMEDIATE;

CREATE VIRTUAL TABLE note_search USING fts5(
    note_id UNINDEXED,
    title,
    content,
    attachments,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO note_search (note_id, title, content)
SELECT notes.id, notes.title,
       COALESCE((
           SELECT group_concat(node.value, ' ')
           FROM json_tree(notes.content_json) AS node
           WHERE node.type = 'text'
       ), '')
FROM notes;

CREATE TABLE note_visits (
    note_id TEXT PRIMARY KEY NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    last_opened_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE attachments (
    id TEXT PRIMARY KEY NOT NULL,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    original_filename TEXT NOT NULL CHECK (length(trim(original_filename)) BETWEEN 1 AND 255),
    stored_filename TEXT NOT NULL UNIQUE,
    mime_type TEXT NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 255),
    size_bytes INTEGER NOT NULL CHECK (size_bytes BETWEEN 1 AND 26214400),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    search_text TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    deleted_at TEXT
);

CREATE INDEX attachments_note_id_idx ON attachments(note_id);
CREATE UNIQUE INDEX attachments_note_sha256_idx
    ON attachments(note_id, sha256) WHERE deleted_at IS NULL;

CREATE TRIGGER attachments_search_insert
AFTER INSERT ON attachments
BEGIN
    UPDATE note_search
    SET attachments = COALESCE((
        SELECT group_concat(search_text, ' ')
        FROM attachments
        WHERE note_id = NEW.note_id AND deleted_at IS NULL
    ), '')
    WHERE note_id = NEW.note_id;
END;

CREATE TRIGGER attachments_search_update
AFTER UPDATE OF deleted_at, search_text ON attachments
BEGIN
    UPDATE note_search
    SET attachments = COALESCE((
        SELECT group_concat(search_text, ' ')
        FROM attachments
        WHERE note_id = NEW.note_id AND deleted_at IS NULL
    ), '')
    WHERE note_id = NEW.note_id;
END;

CREATE TRIGGER attachments_search_delete
AFTER DELETE ON attachments
BEGIN
    UPDATE note_search
    SET attachments = COALESCE((
        SELECT group_concat(search_text, ' ')
        FROM attachments
        WHERE note_id = OLD.note_id AND deleted_at IS NULL
    ), '')
    WHERE note_id = OLD.note_id;
END;

CREATE TABLE note_links (
    source_note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    target_note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    label TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (source_note_id, target_note_id),
    CHECK (source_note_id <> target_note_id)
);

CREATE INDEX note_links_target_id_idx ON note_links(target_note_id);

PRAGMA user_version = 3;
COMMIT;
