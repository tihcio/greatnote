BEGIN IMMEDIATE;

-- Cronologia locale delle richieste AI (facoltativa, disattivabile e
-- cancellabile dalle impostazioni). Delle fonti si conservano solo numero,
-- nota e titolo, non il testo inviato. Le voci collegate a una nota
-- eliminata definitivamente restano, senza collegamento.
CREATE TABLE ai_history (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('chat', 'transform', 'tags', 'tasks', 'search')),
    conversation_id TEXT CHECK (conversation_id IS NULL OR length(conversation_id) <= 64),
    note_id TEXT REFERENCES notes(id) ON DELETE SET NULL,
    prompt TEXT NOT NULL CHECK (length(prompt) <= 4000),
    response TEXT NOT NULL DEFAULT '' CHECK (length(response) <= 100000),
    sources_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(sources_json)),
    model TEXT NOT NULL DEFAULT '' CHECK (length(model) <= 200),
    host TEXT NOT NULL DEFAULT '' CHECK (length(host) <= 300),
    local INTEGER NOT NULL DEFAULT 1 CHECK (local IN (0, 1)),
    error TEXT CHECK (error IS NULL OR length(error) <= 1000),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX ai_history_created ON ai_history (created_at DESC);
CREATE INDEX ai_history_conversation ON ai_history (conversation_id, created_at);

PRAGMA user_version = 10;
COMMIT;
