-- Schéma initial d'Atelier.
-- Conventions : ids TEXT (uuid v7, donc triés chronologiquement),
-- timestamps TEXT ISO-8601 UTC, listes courtes en JSON quand elles sont
-- toujours lues d'un bloc (pas de table de jointure décorative).

PRAGMA foreign_keys = ON;

CREATE TABLE projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    root_path   TEXT,
    git_remote  TEXT,
    color       TEXT NOT NULL DEFAULT '#5eead4',
    zone_x      REAL NOT NULL DEFAULT 0,
    zone_z      REAL NOT NULL DEFAULT 0,
    zone_w      REAL NOT NULL DEFAULT 24,
    zone_d      REAL NOT NULL DEFAULT 18,
    archived    INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL
);

CREATE TABLE agents (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    role          TEXT NOT NULL,
    system_prompt TEXT NOT NULL DEFAULT '',
    skills        TEXT NOT NULL DEFAULT '[]',   -- JSON array
    tools         TEXT NOT NULL DEFAULT '[]',   -- JSON array
    model_ref     TEXT NOT NULL DEFAULT 'reasoning.default',
    archetype     TEXT NOT NULL DEFAULT 'dev',
    enabled       INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_agents_project ON agents(project_id);

-- Permissions. Une ligne = une capacité accordée (ou refusée).
-- L'absence de ligne vaut refus : le système est fail-closed.
CREATE TABLE grants (
    id         TEXT PRIMARY KEY,
    agent_id   TEXT REFERENCES agents(id) ON DELETE CASCADE,   -- NULL = tout le projet
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    tool       TEXT NOT NULL,                                   -- "fs.read", "git.*"
    resource   TEXT NOT NULL,                                   -- JSON ResourceScope
    mode       TEXT NOT NULL                                    -- allow | ask | deny
);
CREATE INDEX idx_grants_agent ON grants(agent_id);
CREATE INDEX idx_grants_project ON grants(project_id);

-- Les étapes sont stockées en JSON : elles sont toujours lues et écrites
-- d'un seul bloc, jamais interrogées individuellement.
CREATE TABLE workflows (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    steps       TEXT NOT NULL DEFAULT '[]',
    trigger     TEXT NOT NULL DEFAULT '{"kind":"manual"}',
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_workflows_project ON workflows(project_id);

CREATE TABLE runs (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    workflow_id TEXT REFERENCES workflows(id) ON DELETE SET NULL,
    title       TEXT NOT NULL,
    request     TEXT,
    status      TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    finished_at TEXT
);
CREATE INDEX idx_runs_project ON runs(project_id, created_at DESC);

CREATE TABLE tasks (
    id          TEXT PRIMARY KEY,
    run_id      TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    agent_id    TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL,
    progress    REAL NOT NULL DEFAULT 0,
    result      TEXT,
    error       TEXT,
    attempt     INTEGER NOT NULL DEFAULT 0,
    position    INTEGER NOT NULL DEFAULT 0,   -- ordre de déclaration, pour l'affichage
    created_at  TEXT NOT NULL,
    started_at  TEXT,
    finished_at TEXT
);
CREATE INDEX idx_tasks_run ON tasks(run_id, position);
CREATE INDEX idx_tasks_agent ON tasks(agent_id, status);
CREATE INDEX idx_tasks_status ON tasks(status);

-- Arêtes du DAG. Table dédiée car interrogée à chaque tick du scheduler.
CREATE TABLE task_deps (
    task_id    TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    depends_on TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, depends_on)
);
CREATE INDEX idx_task_deps_rev ON task_deps(depends_on);

-- Trace d'audit : chaque appel d'outil, sa décision de permission, son résultat.
CREATE TABLE tool_calls (
    id          TEXT PRIMARY KEY,
    task_id     TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    agent_id    TEXT NOT NULL,
    tool        TEXT NOT NULL,
    args        TEXT NOT NULL,
    decision    TEXT NOT NULL,        -- allow | ask | deny
    reason      TEXT NOT NULL DEFAULT '',
    ok          INTEGER,
    output      TEXT,
    duration_ms INTEGER,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_tool_calls_task ON tool_calls(task_id, created_at);

CREATE TABLE approvals (
    id         TEXT PRIMARY KEY,
    agent_id   TEXT NOT NULL,
    task_id    TEXT NOT NULL,
    project_id TEXT NOT NULL,
    tool       TEXT NOT NULL,
    summary    TEXT NOT NULL,
    details    TEXT NOT NULL DEFAULT '',
    resource   TEXT NOT NULL,
    reason     TEXT NOT NULL DEFAULT '',
    resolved   INTEGER,               -- NULL = en attente, 0 = refusé, 1 = accordé
    created_at TEXT NOT NULL
);
CREATE INDEX idx_approvals_pending ON approvals(resolved, created_at);

-- Flux du terminal. Toujours indexé par agent et par tâche : c'est ce qui
-- rend les trois vues (globale / agent / tâche) gratuites.
CREATE TABLE log_lines (
    id         TEXT PRIMARY KEY,
    project_id TEXT,
    agent_id   TEXT,
    task_id    TEXT,
    stream     TEXT NOT NULL,
    text       TEXT NOT NULL,
    ts         TEXT NOT NULL
);
CREATE INDEX idx_logs_agent ON log_lines(agent_id, id);
CREATE INDEX idx_logs_task ON log_lines(task_id, id);

CREATE TABLE memory_entries (
    id         TEXT PRIMARY KEY,
    scope      TEXT NOT NULL,
    kind       TEXT NOT NULL,
    project_id TEXT,
    agent_id   TEXT,
    run_id     TEXT,
    task_id    TEXT,
    content    TEXT NOT NULL,
    importance REAL NOT NULL DEFAULT 0.5,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_memory_lookup ON memory_entries(scope, project_id, agent_id);

-- Recherche plein texte. FTS5 suffit très largement à l'échelle d'un
-- usage personnel, pour zéro appel réseau et zéro dépendance.
-- L'interface de recherche reste abstraite côté Rust : brancher des
-- embeddings plus tard ne changera rien au reste.
CREATE VIRTUAL TABLE memory_fts USING fts5(
    content,
    content='memory_entries',
    content_rowid='rowid'
);
CREATE TRIGGER memory_ai AFTER INSERT ON memory_entries BEGIN
    INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER memory_ad AFTER DELETE ON memory_entries BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, content) VALUES('delete', old.rowid, old.content);
END;
CREATE TRIGGER memory_au AFTER UPDATE ON memory_entries BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, content) VALUES('delete', old.rowid, old.content);
    INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
END;

-- Configuration des fournisseurs IA. Les clés API ne sont PAS ici :
-- elles vont dans le trousseau de l'OS, on ne stocke qu'une référence.
CREATE TABLE provider_configs (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,       -- anthropic | openai | ollama
    label        TEXT NOT NULL,
    base_url     TEXT,
    key_ref      TEXT,                -- identifiant dans le trousseau
    enabled      INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT NOT NULL
);

-- Résolution des alias de modèle : "reasoning.high" -> provider + modèle.
-- C'est ce qui permet de changer de fournisseur sans toucher aux agents.
CREATE TABLE model_routes (
    model_ref   TEXT PRIMARY KEY,     -- "reasoning.high", "classify.fast"
    provider_id TEXT NOT NULL REFERENCES provider_configs(id) ON DELETE CASCADE,
    model       TEXT NOT NULL,
    max_tokens  INTEGER NOT NULL DEFAULT 4096,
    temperature REAL NOT NULL DEFAULT 0.2
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
