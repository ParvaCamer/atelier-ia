-- Dossier de travail d'une tâche : sous-dossier du projet où s'exécutent
-- ses commandes. NULL = la racine du projet, comportement d'avant.
ALTER TABLE tasks ADD COLUMN cwd TEXT;
