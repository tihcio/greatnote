BEGIN IMMEDIATE;

-- Attività autonome o collegate a una nota. La scadenza è una data civile
-- (AAAA-MM-GG) senza fuso: "scade il 14 ottobre" resta tale anche se cambia
-- il fuso del sistema. L'eliminazione è definitiva (con conferma nella UI).
CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL,
    note_id TEXT REFERENCES notes(id) ON DELETE SET NULL,
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 512),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 8192),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'done')),
    priority INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),
    due_date TEXT CHECK (
        due_date IS NULL
        OR (length(due_date) = 10 AND date(due_date) = due_date)
    ),
    completed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((status = 'done') = (completed_at IS NOT NULL))
);

CREATE INDEX tasks_open_due ON tasks (status, due_date);
CREATE INDEX tasks_note ON tasks (note_id);

-- Eventi del calendario locale. Gli eventi con orario hanno istanti UTC e il
-- fuso in cui sono stati creati; quelli di un giorno intero hanno date civili
-- (fine inclusa). `calendar_provider`/`external_id` sono riservati a future
-- integrazioni e restano NULL per gli eventi locali.
CREATE TABLE events (
    id TEXT PRIMARY KEY NOT NULL,
    note_id TEXT REFERENCES notes(id) ON DELETE SET NULL,
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 512),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 8192),
    all_day INTEGER NOT NULL CHECK (all_day IN (0, 1)),
    start_at TEXT,
    end_at TEXT,
    start_date TEXT,
    end_date TEXT,
    timezone TEXT NOT NULL CHECK (length(timezone) BETWEEN 1 AND 64),
    calendar_provider TEXT CHECK (calendar_provider IS NULL OR length(calendar_provider) <= 50),
    external_id TEXT CHECK (external_id IS NULL OR length(external_id) <= 512),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (
        (all_day = 1
            AND start_date IS NOT NULL AND end_date IS NOT NULL
            AND start_at IS NULL AND end_at IS NULL
            AND end_date >= start_date)
        OR (all_day = 0
            AND start_at IS NOT NULL AND end_at IS NOT NULL
            AND start_date IS NULL AND end_date IS NULL
            AND end_at >= start_at)
    )
);

CREATE INDEX events_timed ON events (start_at, end_at);
CREATE INDEX events_all_day ON events (start_date, end_date);

-- Promemoria su note, attività ed eventi. `remind_at` è un istante UTC
-- (AAAA-MM-GGTHH:MM:SSZ); `timezone` e `local_time` conservano l'orario
-- scelto dall'utente per mostrarlo e per i rinvii "a domani".
CREATE TABLE reminders (
    id TEXT PRIMARY KEY NOT NULL,
    entity_type TEXT NOT NULL CHECK (entity_type IN ('note', 'task', 'event')),
    entity_id TEXT NOT NULL,
    remind_at TEXT NOT NULL CHECK (length(remind_at) = 20),
    timezone TEXT NOT NULL CHECK (length(timezone) BETWEEN 1 AND 64),
    local_time TEXT NOT NULL CHECK (length(local_time) = 16),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'fired', 'done')),
    last_triggered_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX reminders_due ON reminders (status, remind_at);
CREATE INDEX reminders_entity ON reminders (entity_type, entity_id);

-- Le note non vengono eliminate fisicamente (cestino); attività ed eventi sì:
-- i loro promemoria spariscono con loro.
CREATE TRIGGER tasks_delete_reminders AFTER DELETE ON tasks BEGIN
    DELETE FROM reminders WHERE entity_type = 'task' AND entity_id = old.id;
END;

CREATE TRIGGER events_delete_reminders AFTER DELETE ON events BEGIN
    DELETE FROM reminders WHERE entity_type = 'event' AND entity_id = old.id;
END;

PRAGMA user_version = 6;
COMMIT;
