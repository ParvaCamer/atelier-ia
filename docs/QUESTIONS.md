# Questions ouvertes

Une ligne par question laissée en suspens par une session autonome :
la question, les options envisagées, et ce qui a été fait en attendant.

- **Lot 1 — coupe d'un skill trop long.** L'énoncé impose `tail`, qui garde la *fin* du skill (limites, compte rendu) et perd le début (rôle). Options : garder la fin / garder le début / refuser à l'enregistrement tout skill > 4 000 caractères. En attendant : `tail` comme demandé, l'enregistrement refuse au-delà de 12 000 caractères et l'éditeur affiche le compteur « n / 4 000 » avec un avertissement.
- **Lot 1 — skill livré supprimé par l'utilisateur.** `ensure_builtin_agent_skills` le recrée au démarrage suivant (il n'insère que les slugs absents). Options : le recréer / mémoriser la suppression / interdire la suppression d'un skill livré. En attendant : recréé (comportement de l'énoncé).
- **Lot 1 — agents du monde initial.** Les agents seedés (« QA Spotly »…) ne sont rattachés à aucun skill de rôle, et seuls `qa` et `dev-front` sont livrés. Options : rattacher automatiquement par rôle / écrire les autres skills (lead, dev-back, designer, marketing, ops, assistant) / laisser l'utilisateur choisir. En attendant : rien d'automatique, choix dans Réglages › Agents.
