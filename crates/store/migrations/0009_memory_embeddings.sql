-- Recherche par proximité de sens : vecteur calculé localement (Ollama),
-- rangé avec le souvenir. Le modèle est noté : deux modèles différents
-- produisent des vecteurs incomparables.
ALTER TABLE memory_entries ADD COLUMN embedding BLOB;
ALTER TABLE memory_entries ADD COLUMN embedding_model TEXT;
