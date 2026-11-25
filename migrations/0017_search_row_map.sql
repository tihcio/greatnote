BEGIN IMMEDIATE;

-- Corrispondenza tra nota e riga dell'indice FTS5. `note_search.note_id` non
-- è indicizzabile (colonna UNINDEXED): cercarla scorre tutto l'indice, a
-- ogni salvataggio e a ogni modifica di un allegato. Con questa tabella
-- aggiornamenti e cancellazioni vanno per rowid. La mantiene l'app (le
-- tabelle FTS5 non hanno trigger); i rowid delle tabelle interne di FTS5
-- restano stabili anche con VACUUM.
CREATE TABLE note_search_rows (
    note_id TEXT PRIMARY KEY NOT NULL,
    search_rowid INTEGER NOT NULL UNIQUE
) WITHOUT ROWID;

INSERT INTO note_search_rows (note_id, search_rowid)
SELECT note_id, MAX(rowid) FROM note_search GROUP BY note_id;

-- Eventuali righe doppie della stessa nota (non dovrebbero esistere).
DELETE FROM note_search
WHERE rowid NOT IN (SELECT search_rowid FROM note_search_rows);

DROP TRIGGER IF EXISTS attachments_search_insert;
DROP TRIGGER IF EXISTS attachments_search_update;
DROP TRIGGER IF EXISTS attachments_search_delete;

CREATE TRIGGER attachments_search_insert
AFTER INSERT ON attachments
BEGIN
    UPDATE note_search
    SET attachments = COALESCE((
        SELECT group_concat(search_text, ' ')
        FROM attachments
        WHERE note_id = NEW.note_id AND deleted_at IS NULL
    ), '')
    WHERE rowid = (SELECT search_rowid FROM note_search_rows WHERE note_id = NEW.note_id);
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
    WHERE rowid = (SELECT search_rowid FROM note_search_rows WHERE note_id = NEW.note_id);
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
    WHERE rowid = (SELECT search_rowid FROM note_search_rows WHERE note_id = OLD.note_id);
END;

PRAGMA user_version = 17;
COMMIT;
