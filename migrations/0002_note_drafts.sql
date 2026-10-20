BEGIN IMMEDIATE;

CREATE TABLE note_drafts (
    note_id TEXT PRIMARY KEY NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (length(title) <= 512),
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    saved_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

PRAGMA user_version = 2;
COMMIT;
