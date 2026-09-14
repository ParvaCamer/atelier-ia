-- Une tâche peut porter une liste de commandes explicites.
-- Liste vide = tâche confiée à un agent IA (phase 3).
-- Liste non vide = tâche déterministe, exécutée sans aucun LLM :
-- `npm test` n'a pas besoin d'un modèle pour être lancé.
ALTER TABLE tasks ADD COLUMN commands TEXT NOT NULL DEFAULT '[]';

-- Étape soumise à validation humaine avant de démarrer.
ALTER TABLE tasks ADD COLUMN requires_approval INTEGER NOT NULL DEFAULT 0;
