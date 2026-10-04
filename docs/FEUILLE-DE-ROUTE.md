# Feuille de route — jusqu'à la version 1

Énoncé destiné à une session de développement autonome qui enchaîne les
lots **dans l'ordre**. Lire `CLAUDE.md` avant de commencer.

## Règles de conduite

1. **Un lot = une branche = un commit final propre.** Nommage :
   `lot-1-skills-agents`, `lot-2-collaboration`, etc. Ne jamais pousser
   sur `main`.
2. **Un lot n'est fini que quand ses critères d'acceptation sont vérifiés
   par des tests qui échouent si on retire la règle protégée.** Un lot sans
   test est un lot non livré.
3. **Ne pas inventer une décision de produit.** Toute question ouverte
   s'écrit dans `docs/QUESTIONS.md` (une ligne : la question, les options,
   ce que tu as fait en attendant), et le travail continue sur le choix le
   plus réversible.
4. **Ne jamais contourner un invariant de `CLAUDE.md`** pour faire passer
   un lot. Préférer livrer moins.
5. **Rendre compte honnêtement** : ce qui est vérifié, ce qui ne l'est pas,
   ce qui reste à voir en local. Ne pas écrire « terminé » pour ce qui n'a
   pas été exécuté.
6. Après chaque lot : `cargo test --workspace --exclude atelier` et
   `pnpm typecheck` doivent passer.

## Ce qui ne peut PAS être fait en cloud — ne pas essayer

- Compiler `src-tauri` ou lancer `pnpm tauri dev` (WebKit, écran).
- Les tests `real_claude`, `real_ollama`, `real_memory` (`#[ignore]`).
- Empaqueter l'application macOS (`.app`, icône, signature, démarrage
  automatique). **Lot réservé au local.**
- Juger un rendu 3D. Le code 3D peut être écrit, son allure se valide en
  local sur captures.

---

## Lot 1 — Skills d'agent

Énoncé détaillé : **`docs/TACHE-skills-agents.md`**. Stockage, injection
budgétée dans le prompt, brouillon validé par l'utilisateur.

## Lot 2 — Suivi d'exécution en direct sur le graphe

**Objectif.** Pendant qu'un run tourne, l'éditeur de workflow montre l'état
réel de chaque étape : en attente, en cours, terminée, échouée, en attente
de validation.

- Le moteur expose déjà `WorldSnapshot` et `RunView` : réutiliser, ne pas
  créer un second chemin de données.
- Le graphe est rendu par `src/ui/workflow/WorldGraph.tsx` (lecture seule
  dans ce mode : pas d'édition pendant une exécution).
- Accès par l'historique (`src/ui/history/History.tsx`) et par le clic sur
  un run en cours.

**Acceptation.** Un run simulé dans le harnais fait changer la couleur des
nœuds ; aucune nouvelle commande IPC n'est nécessaire si le snapshot suffit
— si une est ajoutée, elle est dans `devMock.ts`.

## Lot 3 — Collaboration entre agents visible

**Objectif.** Quand une étape passe le relais à une autre, cela se voit et
se trace : qui a transmis quoi à qui.

- Côté moteur : un événement de passage de relais à la fin d'une tâche qui
  en débloque une autre, consigné et visible dans l'historique.
- Côté monde 3D : un déplacement ou un signal entre les deux agents. Le
  moteur **n'émet aucune coordonnée** — il décrit le relais, `src/world/`
  décide de l'animation.

**Acceptation.** Test moteur : un DAG à deux étapes produit un relais
consigné, avec les deux agents et la tâche source. L'allure visuelle est
validée en local.

## Lot 4 — Fournisseur OpenAI

**Objectif.** Troisième fournisseur derrière l'abstraction existante
(`crates/providers/`), au même niveau que Claude Code et Ollama.

- Clé d'API **jamais** dans le dépôt ni dans le code : saisie dans
  Réglages › IA, stockée en base comme les autres réglages.
- Implémenter `Provider` comme `ollama.rs` : complétion, erreurs typées,
  santé (`provider_health`), modèles disponibles.
- Les alias (`reasoning.high`…) doivent pouvoir pointer dessus sans
  toucher au reste.

**Acceptation.** Tests avec un serveur HTTP simulé (voir `ollama.rs` et ses
tests) : réponse normale, erreur d'authentification, service indisponible.
**Aucun appel réseau réel.**

## Lot 5 — Déclencheurs par événement

**Objectif.** `Trigger::Event` existe dans le domaine mais ne fait rien.
Implémenter **un seul** déclencheur, le plus utile : la surveillance de
fichiers d'un projet.

- Surveiller le dossier du projet, filtrer par motif, déclencher un
  workflow. Anti-rebond obligatoire (plusieurs écritures = un lancement).
- Mêmes garde-fous que les planifications : pas de chevauchement, échéance
  consignée, horloge injectable pour les tests.
- Ne pas surveiller sans dossier de projet défini.

**Acceptation.** Tests avec un dossier temporaire : une rafale d'écritures
ne déclenche qu'un lancement ; un run déjà en cours empêche le suivant.

## Lot 6 — Mémoire : recherche sémantique

**Objectif.** La mémoire ne retrouve aujourd'hui que par mots exacts (FTS5).
Ajouter une recherche par proximité de sens, **en local uniquement**.

- Embeddings via Ollama (`/api/embeddings`), modèle configurable dans les
  réglages. Si Ollama est éteint : repli silencieux sur FTS5, jamais
  d'appel payant.
- Stocker le vecteur avec l'entrée ; calcul de similarité en Rust, sans
  nouvelle dépendance lourde.
- Fusionner les deux classements plutôt que remplacer l'un par l'autre.

**Acceptation.** Test avec des vecteurs fournis à la main (pas d'appel
réseau) : le classement fusionné remonte l'entrée pertinente qu'FTS5 rate ;
Ollama absent ⇒ comportement identique à aujourd'hui.

## Lot 7 — Coût et consommation

**Objectif.** Savoir ce que consomme chaque exécution.

- Les réponses de Claude Code portent déjà `total_cost_usd` et
  `modelUsage` : les consigner par tâche et par run.
- Affichage dans l'historique : coût par run, cumul du mois.
- Aucun appel supplémentaire à un modèle pour mesurer.

**Acceptation.** Test : un run avec un fournisseur simulé qui annonce un
coût produit un cumul correct au niveau du run.

## Lot 8 — Finition de l'interface

À faire en dernier, une fois les fonctionnalités stabilisées.

- États vides et messages d'erreur sur tous les écrans (chargement, vide,
  erreur, succès).
- Fenêtre étroite (~1100 px) : aucun débordement horizontal.
- Raccourcis clavier cohérents, `Échap` qui ne perd jamais une saisie.
- Accessibilité minimale : focus visible, libellés des boutons d'icône.

**Acceptation.** Vérifié dans le harnais navigateur, écran par écran, avec
mesures (pas seulement des captures).

---

## Réservé au local — ne pas tenter en cloud

- **Empaquetage macOS** : `.app`, icône, emplacement stable de la base,
  démarrage au login pour que les planifications tournent.
- **Essai réel** : commande directe, demande en langage naturel, refus
  d'une opération dangereuse, workflow, planification.
- Validation visuelle du monde 3D.
