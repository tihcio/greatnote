BEGIN IMMEDIATE;

-- Testo letto dalle immagini allegate (OCR locale o modello AI con visione).
-- Il testo è aggiunto anche a `search_text`, così entra nella ricerca
-- full-text; l'assistente AI lo usa come parte del testo della nota.
ALTER TABLE attachments ADD COLUMN ocr_text TEXT
    CHECK (ocr_text IS NULL OR length(ocr_text) <= 100000);
ALTER TABLE attachments ADD COLUMN ocr_status TEXT
    CHECK (ocr_status IS NULL OR ocr_status IN ('done', 'failed'));
ALTER TABLE attachments ADD COLUMN ocr_engine TEXT
    CHECK (ocr_engine IS NULL OR length(ocr_engine) <= 200);
ALTER TABLE attachments ADD COLUMN ocr_error TEXT
    CHECK (ocr_error IS NULL OR length(ocr_error) <= 1000);
ALTER TABLE attachments ADD COLUMN ocr_at TEXT;

PRAGMA user_version = 11;
COMMIT;
