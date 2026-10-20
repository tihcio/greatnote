BEGIN IMMEDIATE;

-- Abbonamenti a calendari iCal (indirizzo segreto di Google Calendar,
-- calendario pubblicato di Outlook, ecc.), in sola lettura. L'indirizzo dà
-- accesso al calendario a chiunque lo conosca: resta nel portachiavi del
-- sistema, non qui.
CREATE TABLE calendar_subscriptions (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    color TEXT CHECK (color IS NULL OR length(color) <= 16),
    last_sync_at TEXT,
    last_error TEXT CHECK (last_error IS NULL OR length(last_error) <= 1000),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

ALTER TABLE events ADD COLUMN subscription_id TEXT
    REFERENCES calendar_subscriptions(id) ON DELETE CASCADE;

CREATE UNIQUE INDEX events_subscription ON events (subscription_id, external_id)
    WHERE subscription_id IS NOT NULL;

PRAGMA user_version = 13;
COMMIT;
