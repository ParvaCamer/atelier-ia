-- Clé d'API d'un fournisseur facturé à l'usage (OpenAI). Saisie dans
-- Réglages › IA, stockée dans la base locale de l'utilisateur, jamais
-- renvoyée à l'interface : seul `has_key` sort du moteur.
ALTER TABLE provider_configs ADD COLUMN api_key TEXT;
