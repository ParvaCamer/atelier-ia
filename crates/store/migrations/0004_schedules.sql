-- Planifications : un workflow ou une demande en langage naturel, selon un
-- calendrier cron (5 champs, heure locale).
CREATE TABLE schedules (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    target       TEXT NOT NULL,               -- JSON ScheduleTarget
    -- Dénormalisé depuis `target` pour que la suppression d'un workflow
    -- supprime ses planifications : sinon elles échoueraient à chaque échéance.
    workflow_id  TEXT REFERENCES workflows(id) ON DELETE CASCADE,
    cron         TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    -- Échéance manquée pendant qu'Atelier était fermé : exécuter une fois au
    -- prochain lancement, ou ignorer.
    run_missed   INTEGER NOT NULL DEFAULT 1,
    last_run_at  TEXT,
    last_run_id  TEXT,
    last_outcome TEXT,                         -- launched | skipped | error
    last_error   TEXT,
    next_run_at  TEXT,
    created_at   TEXT NOT NULL
);
CREATE INDEX idx_schedules_due ON schedules(enabled, next_run_at);

ALTER TABLE runs ADD COLUMN schedule_id TEXT REFERENCES schedules(id) ON DELETE SET NULL;
CREATE INDEX idx_runs_schedule ON runs(schedule_id);
