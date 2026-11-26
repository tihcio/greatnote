BEGIN IMMEDIATE;

-- Titolo e testo delle ultime versioni sincronizzate di ogni nota, con la
-- versione da cui ciascuna è partita (ADR 0034). Servono alla fusione a tre
-- vie di modifiche contemporanee: la versione di partenza comune e la
-- verifica che sia davvero un antenato della versione locale. Dati del
-- protocollo, ricostruibili: niente vincoli verso le note, vengono
-- cancellati con la storia della sincronizzazione.
CREATE TABLE sync_note_bases (
    note_id TEXT NOT NULL,
    counter INTEGER NOT NULL,
    device TEXT NOT NULL,
    base_counter INTEGER,
    base_device TEXT,
    title TEXT NOT NULL,
    content_json TEXT NOT NULL,
    stored_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (note_id, counter, device)
) WITHOUT ROWID;

PRAGMA user_version = 19;
COMMIT;
