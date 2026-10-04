-- Tableau de l'orchestrateur : demandes en attente d'exécution, posées par
-- l'utilisateur, par l'orchestrateur ou proposées par un chef de projet.
CREATE TABLE todos (
    id          TEXT PRIMARY KEY,
    text        TEXT NOT NULL,
    project_id  TEXT REFERENCES projects(id) ON DELETE CASCADE,
    author      TEXT NOT NULL,                -- JSON TodoAuthor
    status      TEXT NOT NULL,
    depth       INTEGER NOT NULL DEFAULT 0,
    run_id      TEXT REFERENCES runs(id) ON DELETE SET NULL,
    note        TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX idx_todos_status ON todos(status, created_at);
