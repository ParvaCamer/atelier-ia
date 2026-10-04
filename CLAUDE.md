# Atelier — consignes de travail

Application desktop **personnelle** d'orchestration d'agents IA, affichée
comme un petit monde 3D. Jamais publiée, jamais distribuée. L'intelligence
passe par **Claude Code en headless** (abonnement personnel) et **Ollama**
en local : il n'y a aucune clé d'API dans ce dépôt, et il ne doit jamais y
en avoir.

Architecture détaillée : **docs/ARCHITECTURE.md**. À lire avant toute
modification du moteur.

## Invariants — ne jamais les contourner

1. **Aucun crate de `crates/` ne dépend de Tauri.** `src-tauri/` est un
   adaptateur mince : il appelle le moteur, il ne décide rien.
2. **La 3D est une projection.** Le moteur n'émet aucune coordonnée ;
   supprimer `src/world/` doit laisser une application fonctionnelle.
3. **Les règles métier vivent dans le moteur**, principalement
   `crates/engine/src/config.rs`. L'interface affiche les erreurs du
   moteur, elle ne valide rien elle-même : une règle qui ne vivrait que
   dans l'UI serait contournable et finirait par diverger.
4. **Fail-closed.** L'absence de règle de permission vaut refus. Toute
   modification de `crates/permissions/` doit être couverte par un test.
5. **Aucun LLM dans le scheduler.** Un modèle planifie une fois, du code
   ordinaire exécute. C'est ce qui rend le système reprenable et débuggable.
6. **Pas de commit ni de push sans demande explicite** de l'utilisateur.

## Commandes

```bash
pnpm install
cargo test --workspace     # moteur, persistance, permissions (~130 tests)
pnpm typecheck             # frontend
pnpm bindings              # régénère src/ipc/generated depuis les types Rust
pnpm dev                   # harnais navigateur, sans Rust : http://localhost:1420
pnpm tauri dev             # vraie application — macOS uniquement ici
```

## Ce qui ne marche PAS dans une session cloud (Linux, sans écran)

- **`pnpm tauri dev` et toute compilation de `src-tauri`** : nécessitent
  WebKit et un environnement graphique. Sur Linux, travailler le moteur
  avec `cargo test -p atelier-engine` (ou `--workspace --exclude atelier`),
  pas avec `--workspace` complet.
- **Les tests réels** `real_claude`, `real_ollama`, `real_memory` : ils sont
  `#[ignore]` et exigent un Claude Code connecté ou un Ollama local.
  **Ne pas les lancer** — ils échoueront, et `real_claude` consomme du quota.
- **La vérification visuelle** passe par le harnais navigateur (`pnpm dev`),
  qui simule l'IPC avec `src/ipc/devMock.ts`. Toute nouvelle commande IPC
  doit y être ajoutée, sinon l'interface casse hors application.

## Pièges connus

- **Ne jamais lancer `cargo` pendant que `pnpm tauri dev` tourne** : les
  deux écrivent dans le même `target/`, le cache se corrompt et l'édition
  de liens échoue (`ld: symbol(s) not found`). Réparation :
  `cargo clean -p atelier`.
- **Après modification d'un type `#[ts(export)]` dans `crates/domain`**,
  lancer `pnpm bindings`. Les types générés sont commités : un oubli fait
  compiler le frontend contre des types périmés.
- **`target/` atteint vite 15 Go** et le disque de la machine de dev est
  presque plein. Ne pas multiplier les profils de compilation.
- `crates/engine/src/memory.rs` et `tests/phase4.rs` contiennent de fausses
  clés (`sk-ant-...`) : ce sont les tests du filtre anti-secrets, pas des
  fuites.

## Skills d'agent — un par rôle

Atelier est une application d'orchestration : **le comportement d'un agent
est défini par un skill, pas par un prompt recopié ni par une consigne
noyée dans le code.**

**Deux niveaux, jamais mélangés :**

- **Le skill de rôle** — le métier (QA, Dev Front, Ops, Marketing…),
  partagé par tous les projets. Les rôles livrés avec l'application vivent
  dans `skills/agents/<role>.md`, versionnés ici.
- **La surcouche d'agent** — quelques lignes propres à un agent précis.
  Elle ne redit pas le métier.

Un même métier n'est donc jamais dupliqué entre projets : « QA Spotly » et
« QA Agency » partagent le skill de rôle et ne diffèrent que par leur
surcouche.

**Production d'un skill : brouillon proposé, validé par l'utilisateur.**
À la création d'un agent, le moteur génère un brouillon à partir du rôle,
l'affiche dans les réglages, et l'utilisateur l'enregistre après relecture.
Un appel au modèle par création, jamais pendant le travail.

**Un agent ne réécrit jamais son propre skill.** Un contrat de
comportement qui change pendant l'exécution rend les tâches passées
inexplicables : on ne sait plus avec quelles consignes elles ont tourné.
La modification est un acte humain, visible, enregistré.

Les permissions restent du ressort du moteur : **un skill n'accorde aucun
droit**, il décrit une méthode.

**État actuel : rien de tout ça n'est implémenté.** Les agents portent un
`system_prompt` et une liste d'étiquettes `skills` en base. Le stockage,
la génération du brouillon et l'injection au lancement d'une tâche restent
à écrire. Écrire quand même les skills de rôle dans `skills/agents/` :
c'est le contrat visé.

## Style

- **Commentaires et messages d'erreur en français**, comme tout le dépôt.
  Un commentaire explique *pourquoi*, jamais *quoi*.
- Les tests du moteur sont groupés par phase : `execution.rs`,
  `intelligence.rs`, `config.rs`, `phase4.rs`. Ajouter au bon fichier.
- Les messages d'erreur sont lus par l'utilisateur dans l'interface :
  ils nomment l'objet fautif et disent quoi faire.
