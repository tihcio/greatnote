BEGIN IMMEDIATE;

-- Account collegati di calendari esterni (Google Calendar, Microsoft
-- Outlook/365). I token non sono qui: il token di aggiornamento resta nel
-- portachiavi del sistema, indicizzato dall'id dell'account.
CREATE TABLE calendar_accounts (
    id TEXT PRIMARY KEY NOT NULL,
    provider TEXT NOT NULL CHECK (provider IN ('google', 'microsoft')),
    -- Indirizzo o nome mostrato all'utente.
    label TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 320),
    last_sync_at TEXT,
    last_error TEXT CHECK (last_error IS NULL OR length(last_error) <= 1000),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (provider, label)
);

-- Calendari di un account. Solo quelli selezionati vengono sincronizzati.
CREATE TABLE external_calendars (
    id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES calendar_accounts(id) ON DELETE CASCADE,
    remote_id TEXT NOT NULL CHECK (length(remote_id) BETWEEN 1 AND 1024),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 512),
    color TEXT CHECK (color IS NULL OR length(color) <= 16),
    can_edit INTEGER NOT NULL DEFAULT 0 CHECK (can_edit IN (0, 1)),
    is_primary INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    selected INTEGER NOT NULL DEFAULT 0 CHECK (selected IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (account_id, remote_id)
);

-- Gli eventi esterni sono una copia locale del calendario d'origine, che
-- resta la fonte di verità: si modificano tramite le API del fornitore e la
-- sincronizzazione li aggiorna. `calendar_provider` ed `external_id`
-- esistono dalla migrazione 6.
ALTER TABLE events ADD COLUMN external_calendar_id TEXT
    REFERENCES external_calendars(id) ON DELETE CASCADE;
ALTER TABLE events ADD COLUMN etag TEXT CHECK (etag IS NULL OR length(etag) <= 512);
ALTER TABLE events ADD COLUMN web_link TEXT CHECK (web_link IS NULL OR length(web_link) <= 2048);
-- Occorrenze di eventi ricorrenti e calendari in sola lettura.
ALTER TABLE events ADD COLUMN read_only INTEGER NOT NULL DEFAULT 0 CHECK (read_only IN (0, 1));

CREATE UNIQUE INDEX events_external ON events (external_calendar_id, external_id)
    WHERE external_calendar_id IS NOT NULL;

PRAGMA user_version = 12;
COMMIT;
