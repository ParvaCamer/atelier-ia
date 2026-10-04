# Skills de rôle

Un fichier par **rôle** d'agent (`<role>.md`) : le métier, partagé par tous
les projets — expertise, méthode, limites, format de compte rendu. Les
spécificités d'un agent précis vont dans sa surcouche, pas ici.

La règle complète est dans [CLAUDE.md](../../CLAUDE.md). Ces fichiers sont
embarqués dans le binaire (`include_str!`, `crates/store/src/seed.rs`) et
insérés en base au démarrage **seulement si leur slug est absent** : une
version modifiée par l'utilisateur n'est jamais écrasée. Ajouter un rôle
livré = ajouter le fichier et sa ligne dans `BUILTIN_AGENT_SKILLS`.

Le titre du skill est son premier titre `# …`. Le prompt n'en injecte que
les 4 000 derniers caractères : rester court.
