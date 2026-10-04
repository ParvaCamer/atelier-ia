-- Passages de relais : une tâche terminée en débloque une autre. Consigné
-- par le moteur (jamais par un agent) : qui a transmis quoi à qui.
CREATE TABLE handoffs (
    id         TEXT PRIMARY KEY,
    run_id     TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    from_task  TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    to_task    TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    from_agent TEXT NOT NULL,
    to_agent   TEXT NOT NULL,
    summary    TEXT NOT NULL DEFAULT '',   -- extrait du résultat transmis
    created_at TEXT NOT NULL
);
CREATE INDEX idx_handoffs_run ON handoffs(run_id, created_at);
