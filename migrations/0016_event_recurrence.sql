BEGIN IMMEDIATE;

-- Eventi ricorrenti: regola (`domain::recurrence::Recurrence` in JSON,
-- validata in Rust) a partire dalla data d'inizio nel fuso dell'evento, e
-- date (`AAAA-MM-GG`) delle occorrenze tolte dalla serie, in un array JSON.
-- Le occorrenze non sono righe: vengono calcolate quando si chiede un
-- intervallo di date.
ALTER TABLE events ADD COLUMN recurrence TEXT CHECK (
    recurrence IS NULL OR (length(recurrence) <= 512 AND json_valid(recurrence))
);
ALTER TABLE events ADD COLUMN recurrence_exceptions TEXT CHECK (
    recurrence_exceptions IS NULL
    OR (length(recurrence_exceptions) <= 40000 AND json_valid(recurrence_exceptions))
);

-- Promemoria ricorrenti sulle note: quando uno scatta nasce il successivo,
-- con un id derivato da questo (nessun doppione tra dispositivi).
ALTER TABLE reminders ADD COLUMN recurrence TEXT CHECK (
    recurrence IS NULL OR (length(recurrence) <= 512 AND json_valid(recurrence))
);

PRAGMA user_version = 16;
COMMIT;
