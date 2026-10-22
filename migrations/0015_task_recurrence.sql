BEGIN IMMEDIATE;

-- Ripetizione delle attività (`domain::recurrence::Recurrence` in JSON,
-- validata in Rust). Completando un'attività ricorrente nasce la successiva
-- con un id derivato da questa, così due dispositivi che completano la stessa
-- attività non creano doppioni.
ALTER TABLE tasks ADD COLUMN recurrence TEXT CHECK (
    recurrence IS NULL OR (length(recurrence) <= 512 AND json_valid(recurrence))
);

PRAGMA user_version = 15;
COMMIT;
