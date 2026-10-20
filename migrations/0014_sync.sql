BEGIN IMMEDIATE;

-- Sincronizzazione tra dispositivi tramite cartella condivisa (ADR 0017).

-- Entità modificate localmente e non ancora esportate.
CREATE TABLE sync_dirty (
    entity TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    PRIMARY KEY (entity, entity_key)
) WITHOUT ROWID;

-- Durante l'applicazione di modifiche remote i trigger non registrano nulla.
CREATE TABLE sync_control (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    applying INTEGER NOT NULL DEFAULT 0 CHECK (applying IN (0, 1))
);
INSERT INTO sync_control (id, applying) VALUES (1, 0);

-- Versione (orologio di Lamport, dispositivo) di ogni entità sincronizzata.
CREATE TABLE sync_versions (
    entity TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    counter INTEGER NOT NULL CHECK (counter >= 1),
    device TEXT NOT NULL CHECK (length(device) BETWEEN 1 AND 64),
    PRIMARY KEY (entity, entity_key)
) WITHOUT ROWID;

-- Lotti già registrati nel database e da scrivere nella cartella: salvati
-- nella stessa transazione delle versioni, scritti dopo il commit e
-- riscritti al giro successivo se la scrittura si interrompe.
CREATE TABLE sync_outbox (
    seq INTEGER PRIMARY KEY CHECK (seq >= 1),
    batch_json TEXT NOT NULL CHECK (json_valid(batch_json))
);

-- Ultimo lotto applicato per ogni dispositivo (incluso questo).
CREATE TABLE sync_devices (
    device_id TEXT PRIMARY KEY NOT NULL CHECK (length(device_id) BETWEEN 1 AND 64),
    name TEXT NOT NULL DEFAULT '' CHECK (length(name) <= 200),
    applied_seq INTEGER NOT NULL DEFAULT 0 CHECK (applied_seq >= 0),
    last_seen_at TEXT
);

-- Stato del protocollo (identità, contatori, ultimo esito).
CREATE TABLE sync_state (
    key TEXT PRIMARY KEY NOT NULL CHECK (length(key) BETWEEN 1 AND 100),
    value TEXT NOT NULL CHECK (length(value) <= 4096)
);

-- Versioni perdenti di modifiche contemporanee, conservate per l'utente.
CREATE TABLE sync_conflicts (
    id TEXT PRIMARY KEY NOT NULL,
    entity TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '' CHECK (length(title) <= 512),
    data_json TEXT NOT NULL CHECK (json_valid(data_json)),
    from_device TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    dismissed_at TEXT
);

-- Allegati il cui contenuto non è ancora arrivato nella cartella.
CREATE TABLE sync_missing_blobs (
    stored_filename TEXT PRIMARY KEY NOT NULL,
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64)
);

-- Id remoti uniti a entità locali equivalenti (tag con lo stesso nome).
CREATE TABLE sync_aliases (
    entity TEXT NOT NULL,
    remote_key TEXT NOT NULL,
    local_key TEXT NOT NULL,
    PRIMARY KEY (entity, remote_key)
) WITHOUT ROWID;

-- Note il cui testo è stato cambiato da un altro dispositivo: un salvataggio
-- dell'editor basato su una versione precedente conserva quel testo.
CREATE TABLE sync_remote_edits (
    note_id TEXT PRIMARY KEY NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    applied_at TEXT NOT NULL
);

CREATE TRIGGER sync_spaces_insert AFTER INSERT ON spaces
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'spaces', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'spaces' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_spaces_update AFTER UPDATE ON spaces
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'spaces', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'spaces' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_spaces_delete AFTER DELETE ON spaces
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'spaces', OLD.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'spaces' AND entity_key = OLD.id);
END;

CREATE TRIGGER sync_notebooks_insert AFTER INSERT ON notebooks
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'notebooks', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'notebooks' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_notebooks_update AFTER UPDATE ON notebooks
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'notebooks', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'notebooks' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_notebooks_delete AFTER DELETE ON notebooks
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'notebooks', OLD.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'notebooks' AND entity_key = OLD.id);
END;

CREATE TRIGGER sync_tags_insert AFTER INSERT ON tags
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'tags', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'tags' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_tags_update AFTER UPDATE ON tags
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'tags', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'tags' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_tags_delete AFTER DELETE ON tags
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'tags', OLD.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'tags' AND entity_key = OLD.id);
END;

CREATE TRIGGER sync_notes_insert AFTER INSERT ON notes
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'notes', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'notes' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_notes_update AFTER UPDATE ON notes
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'notes', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'notes' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_notes_delete AFTER DELETE ON notes
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'notes', OLD.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'notes' AND entity_key = OLD.id);
END;

CREATE TRIGGER sync_note_tags_insert AFTER INSERT ON note_tags
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'note_tags', NEW.note_id || '|' || NEW.tag_id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'note_tags' AND entity_key = NEW.note_id || '|' || NEW.tag_id);
END;

CREATE TRIGGER sync_note_tags_update AFTER UPDATE ON note_tags
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'note_tags', NEW.note_id || '|' || NEW.tag_id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'note_tags' AND entity_key = NEW.note_id || '|' || NEW.tag_id);
END;

CREATE TRIGGER sync_note_tags_delete AFTER DELETE ON note_tags
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'note_tags', OLD.note_id || '|' || OLD.tag_id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'note_tags' AND entity_key = OLD.note_id || '|' || OLD.tag_id);
END;

CREATE TRIGGER sync_note_links_insert AFTER INSERT ON note_links
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'note_links', NEW.source_note_id || '|' || NEW.target_note_id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'note_links' AND entity_key = NEW.source_note_id || '|' || NEW.target_note_id);
END;

CREATE TRIGGER sync_note_links_update AFTER UPDATE ON note_links
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'note_links', NEW.source_note_id || '|' || NEW.target_note_id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'note_links' AND entity_key = NEW.source_note_id || '|' || NEW.target_note_id);
END;

CREATE TRIGGER sync_note_links_delete AFTER DELETE ON note_links
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'note_links', OLD.source_note_id || '|' || OLD.target_note_id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'note_links' AND entity_key = OLD.source_note_id || '|' || OLD.target_note_id);
END;

CREATE TRIGGER sync_attachments_insert AFTER INSERT ON attachments
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'attachments', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'attachments' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_attachments_update AFTER UPDATE ON attachments
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'attachments', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'attachments' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_attachments_delete AFTER DELETE ON attachments
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'attachments', OLD.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'attachments' AND entity_key = OLD.id);
END;

CREATE TRIGGER sync_tasks_insert AFTER INSERT ON tasks
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'tasks', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'tasks' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_tasks_update AFTER UPDATE ON tasks
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'tasks', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'tasks' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_tasks_delete AFTER DELETE ON tasks
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'tasks', OLD.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'tasks' AND entity_key = OLD.id);
END;

CREATE TRIGGER sync_events_insert AFTER INSERT ON events
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
    AND NEW.calendar_provider IS NULL
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'events', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'events' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_events_update AFTER UPDATE ON events
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
    AND NEW.calendar_provider IS NULL
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'events', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'events' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_events_delete AFTER DELETE ON events
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
    AND OLD.calendar_provider IS NULL
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'events', OLD.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'events' AND entity_key = OLD.id);
END;

CREATE TRIGGER sync_reminders_insert AFTER INSERT ON reminders
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
    AND (NEW.entity_type <> 'event' OR EXISTS (SELECT 1 FROM events WHERE events.id = NEW.entity_id AND events.calendar_provider IS NULL))
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'reminders', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'reminders' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_reminders_update AFTER UPDATE ON reminders
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
    AND (NEW.entity_type <> 'event' OR EXISTS (SELECT 1 FROM events WHERE events.id = NEW.entity_id AND events.calendar_provider IS NULL))
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'reminders', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'reminders' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_reminders_delete AFTER DELETE ON reminders
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
    AND (OLD.entity_type <> 'event' OR EXISTS (SELECT 1 FROM events WHERE events.id = OLD.entity_id AND events.calendar_provider IS NULL))
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'reminders', OLD.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'reminders' AND entity_key = OLD.id);
END;

CREATE TRIGGER sync_templates_insert AFTER INSERT ON templates
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'templates', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'templates' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_templates_update AFTER UPDATE ON templates
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'templates', NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'templates' AND entity_key = NEW.id);
END;

CREATE TRIGGER sync_templates_delete AFTER DELETE ON templates
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'templates', OLD.id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'templates' AND entity_key = OLD.id);
END;

CREATE TRIGGER sync_trash_relations_insert AFTER INSERT ON trash_relations
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'trash_relations', NEW.item_type || '|' || NEW.item_id || '|' || NEW.related_id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'trash_relations' AND entity_key = NEW.item_type || '|' || NEW.item_id || '|' || NEW.related_id);
END;

CREATE TRIGGER sync_trash_relations_update AFTER UPDATE ON trash_relations
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'trash_relations', NEW.item_type || '|' || NEW.item_id || '|' || NEW.related_id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'trash_relations' AND entity_key = NEW.item_type || '|' || NEW.item_id || '|' || NEW.related_id);
END;

CREATE TRIGGER sync_trash_relations_delete AFTER DELETE ON trash_relations
WHEN (SELECT applying FROM sync_control WHERE id = 1) = 0
BEGIN
    INSERT INTO sync_dirty (entity, entity_key)
    SELECT 'trash_relations', OLD.item_type || '|' || OLD.item_id || '|' || OLD.related_id
    WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE entity = 'trash_relations' AND entity_key = OLD.item_type || '|' || OLD.item_id || '|' || OLD.related_id);
END;

PRAGMA user_version = 14;
COMMIT;
