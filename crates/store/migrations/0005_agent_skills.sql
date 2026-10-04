-- Skills de rôle : le métier d'un agent (QA, Dev Front…), partagé entre
-- projets. La surcouche propre à un agent vit dans `agents.skill_notes`.
-- Seul l'écran de réglages écrit ici : aucun outil d'agent n'y a accès.
CREATE TABLE agent_skills (
  slug       TEXT PRIMARY KEY,   -- "qa", "dev-front"
  title      TEXT NOT NULL,
  content    TEXT NOT NULL,      -- markdown
  origin     TEXT NOT NULL,      -- 'builtin' | 'user'
  updated_at TEXT NOT NULL
);
ALTER TABLE agents ADD COLUMN skill_slug  TEXT REFERENCES agent_skills(slug);
ALTER TABLE agents ADD COLUMN skill_notes TEXT NOT NULL DEFAULT '';
