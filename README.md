# Atelier

Centre de contrôle personnel pour workflows pilotés par agents IA,
représenté comme un petit monde 3D vivant.

> L'architecture complète (stack, modèle de données, orchestrateur,
> permissions, mémoire, risques) est dans **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**.

## Démarrer

```bash
pnpm install
pnpm tauri dev
```

Travailler l'interface **sans recompiler Rust** (harnais navigateur, données
factices, moteur absent) :

```bash
pnpm dev          # http://localhost:1420
```

## Utiliser

La barre du haut a deux modes (clic sur le symbole pour basculer) :

- **◆ Demande** — langage naturel. L'orchestrateur choisit le projet
  (l'agent sélectionné sert d'indice), planifie, répartit entre agents.
- **$ Commande** — commande exacte pour l'agent sélectionné, sans LLM.

L'intelligence passe par **Claude Code** déjà connecté sur la machine
(abonnement, usage personnel). **Ollama** est facultatif : sans lui,
l'aiguillage se replie sur Claude Code. Pour épargner le quota :

```bash
brew install ollama && ollama pull llama3.2
```

## Vérifier

```bash
cargo test --workspace   # moteur, persistance, permissions
pnpm typecheck           # frontend
pnpm bindings            # régénère les types TS depuis Rust
# mesures réelles avec Ollama (gratuit)
cargo test -p atelier-engine --test real_ollama -- --ignored --nocapture
cargo test -p atelier-engine --test real_memory -- --ignored --nocapture
# bout-en-bout avec le vrai Claude Code (consomme du quota)
cargo test -p atelier-engine --test real_claude -- --ignored --nocapture
```

## Organisation

```
crates/          moteur — aucune dépendance à Tauri, volontairement
  domain/        types purs, zéro I/O
  bus/           bus d'événements
  store/         SQLite, migrations, repositories
  permissions/   moteur de politique (fail-closed)
  providers/     alias de modèle → Claude Code CLI / Ollama
  tools/         fs, shell (pipes), pty (terminal interactif)
  engine/        état du monde, scheduler, porte de permissions, journaux
src-tauri/       coquille desktop (adaptateur mince)
src/
  ipc/           bindings typés générés depuis Rust
  state/         store zustand — pont React ⇄ 3D
  world/         Three.js, aucun import React
  ui/            React, aucun import three
```

## Règles structurantes

1. **La 3D est une projection, jamais une source de vérité.** Le moteur
   n'émet aucune coordonnée ; supprimer `src/world/` doit laisser une
   application fonctionnelle.
2. **Aucun crate de `crates/` ne dépend de Tauri.** La coquille desktop
   est remplaçable ; le moteur ne l'est pas.
3. **Fail-closed.** L'absence de règle de permission vaut refus.
4. **Aucun LLM dans le scheduler.** Un modèle planifie une fois, du code
   ordinaire exécute — c'est ce qui rend le système reprenable et débuggable.

## État

| Phase | Contenu | État |
|---|---|---|
| 0 | Socle : workspace, domaine, SQLite, bus, moteur, IPC typée | ✅ |
| 1 | Monde 3D, zones, agents, états visuels, HUD, popover, terminal | ✅ |
| 2 | PTY réel, outils fs/shell, permissions appliquées, approbations, scheduler DAG, pause/stop/relance | ✅ |
| 3 | Fournisseurs (Claude Code, Ollama), runtime d'agent, orchestrateur | ✅ |
| 4 | Mémoire structurée (extraction filtrée), historique, planifications | ✅ |
| V1 | Éditeur visuel de workflows (graphe de dépendances, diagnostic du moteur) | ✅ |

Ce qui s'exécute sans appeler un modèle : commandes directes confiées à un
agent, et workflows dont les étapes portent des commandes explicites. Les
étapes rédigées en langage naturel passent par Claude Code, et échouent
explicitement si aucun fournisseur n'est joignable.

Les consignes de travail pour un agent (invariants, commandes, limites en
session cloud, pièges) sont dans **[CLAUDE.md](CLAUDE.md)**.
