BEGIN IMMEDIATE;

-- Richieste distruttive arrivate dal server MCP (spostare una nota nel
-- cestino, riscriverne il testo). Il server le accoda soltanto: le applica
-- l'app dopo la conferma dell'utente, che vede l'anteprima (ADR 0014).
-- Restano solo sul dispositivo: la sincronizzazione non le copia.
CREATE TABLE mcp_requests (
    id TEXT PRIMARY KEY NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('trash_note', 'replace_note')),
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    -- Nuovo titolo e contenuto proposti (solo per replace_note).
    title TEXT,
    content_json TEXT,
    -- Motivo scritto dal client, mostrato all'utente.
    reason TEXT NOT NULL DEFAULT '',
    -- Versione della nota quando è arrivata la richiesta.
    base_updated_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'approved', 'rejected', 'expired', 'failed')),
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    decided_at TEXT
);

CREATE INDEX mcp_requests_status ON mcp_requests (status, created_at);
CREATE INDEX mcp_requests_note ON mcp_requests (note_id);

PRAGMA user_version = 18;
COMMIT;
