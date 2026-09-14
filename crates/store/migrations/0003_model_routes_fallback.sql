-- Repli d'une route de modèle vers une autre quand son fournisseur est
-- indisponible (ex. Ollama non lancé → Claude Code). Explicite par route :
-- un repli silencieux vers un fournisseur au quota limité serait une surprise.
ALTER TABLE model_routes ADD COLUMN fallback_ref TEXT;
