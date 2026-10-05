-- Rendus visuels : images créées ou modifiées par une tâche dans le dossier
-- du projet, relevées par le moteur à la fin de la tâche.
CREATE TABLE renders (
    id          TEXT PRIMARY KEY,
    run_id      TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    path        TEXT NOT NULL,                -- relatif au dossier du projet
    size_bytes  INTEGER NOT NULL,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_renders_created ON renders(created_at);
CREATE INDEX idx_renders_task ON renders(task_id);

-- Aperçu en direct d'un projet : l'adresse de son site (local ou en ligne).
ALTER TABLE projects ADD COLUMN preview_url TEXT;
