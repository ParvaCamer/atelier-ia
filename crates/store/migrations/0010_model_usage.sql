-- Consommation : une ligne par appel à un modèle, telle qu'annoncée par le
-- fournisseur. Aucune mesure supplémentaire. Le coût est vide quand le
-- fournisseur n'en donne pas (OpenAI ne renvoie que des jetons).
CREATE TABLE model_usage (
    id            TEXT PRIMARY KEY,
    run_id        TEXT REFERENCES runs(id) ON DELETE CASCADE,
    task_id       TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    purpose       TEXT NOT NULL,            -- agent | planning | routing | memory | draft | test
    served_by     TEXT NOT NULL,
    input_tokens  INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd      REAL,
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_usage_run ON model_usage(run_id);
CREATE INDEX idx_usage_task ON model_usage(task_id);
CREATE INDEX idx_usage_time ON model_usage(created_at);
