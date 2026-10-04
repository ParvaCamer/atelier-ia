-- Surveillance de fichiers : un workflow lancé quand des fichiers du projet
-- correspondant à des motifs changent. Mêmes garde-fous que les
-- planifications (pas de chevauchement, issue consignée).
CREATE TABLE file_watches (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    workflow_id   TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    patterns      TEXT NOT NULL DEFAULT '[]',   -- JSON, motifs relatifs au dossier du projet
    debounce_secs INTEGER NOT NULL DEFAULT 5,
    enabled       INTEGER NOT NULL DEFAULT 1,
    last_run_at   TEXT,
    last_run_id   TEXT,
    last_outcome  TEXT,                         -- launched | skipped | error
    last_error    TEXT,
    last_trigger  TEXT,                         -- fichier(s) à l'origine du dernier passage
    created_at    TEXT NOT NULL
);
