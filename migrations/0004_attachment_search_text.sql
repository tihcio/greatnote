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

UPDATE attachments
SET search_text = original_filename
WHERE search_text = '';

PRAGMA user_version = 4;
