# Tâche — skills d'agent : stockage, brouillon, injection

Énoncé destiné à une session de développement autonome. Lire d'abord
`CLAUDE.md` (invariants, limites de l'environnement) et la section
« Skills d'agent » qu'il contient, puis `docs/ARCHITECTURE.md` §H.

## Où travailler

**Tout le code de cette tâche est dans le dépôt `atelier`.** Si la session
est ouverte sur le dossier parent `~/Desktop/Dev` (pour garder les autres
projets sous la main), commencer par se placer dans `atelier/` : tous les
chemins de cet énoncé lui sont relatifs, et les commandes (`cargo`, `pnpm`)
doivent être lancées depuis là.

Les dossiers voisins (Spotly, Nuea, Site perso…) ne sont **ni versionnés,
ni liés à ce dépôt** : ils servent éventuellement d'exemples de projets
réels, jamais de code à modifier dans cette tâche. Dans une session cloud,
ils sont tout simplement absents.

## Objectif

Aujourd'hui le comportement d'un agent tient dans un champ `system_prompt`
et une liste d'étiquettes `skills`. On veut un **skill de rôle** —
le métier, partagé entre projets — et une **surcouche** propre à chaque
agent, tous deux injectés dans le prompt au lancement d'une tâche.

Trois briques, dans cet ordre : **stockage**, **injection**, **brouillon**.
Les deux premières n'appellent aucun modèle et sont entièrement testables.

## Décisions déjà prises — ne pas les rouvrir

1. **Un skill par rôle**, pas par agent. « QA Spotly » et « QA Agency »
   partagent le skill `qa` et ne diffèrent que par leur surcouche.
2. **Le brouillon est proposé, jamais appliqué seul.** La génération
   renvoie du texte à l'interface ; seul un enregistrement explicite de
   l'utilisateur écrit en base.
3. **Un agent ne réécrit jamais son skill.** Aucun outil, aucune action
   du runtime ne doit pouvoir écrire dans ces tables — comme pour les
   permissions.
4. **Un skill n'accorde aucun droit.** Les `Grant` restent seuls juges.
5. Les skills livrés sont **embarqués dans le binaire** (`include_str!`),
   pas lus depuis le disque à l'exécution : l'application packagée n'a pas
   le dépôt à côté d'elle.

## 1. Stockage

**Migration `crates/store/migrations/0005_agent_skills.sql`** — ne jamais
modifier une migration existante.

```sql
CREATE TABLE agent_skills (
  slug       TEXT PRIMARY KEY,   -- "qa", "dev-front"
  title      TEXT NOT NULL,
  content    TEXT NOT NULL,      -- markdown
  origin     TEXT NOT NULL,      -- 'builtin' | 'user'
  updated_at TEXT NOT NULL
);
ALTER TABLE agents ADD COLUMN skill_slug  TEXT REFERENCES agent_skills(slug);
ALTER TABLE agents ADD COLUMN skill_notes TEXT NOT NULL DEFAULT '';
```

Le champ `skills` existant (étiquettes) reste en place : il sert encore à
l'orchestrateur pour le catalogue d'équipe. Ne pas le supprimer dans cette
tâche.

- **Domaine** : `AgentSkill` dans `crates/domain/src/agent.rs`
  (`#[ts(export)]`, `rename_all = "camelCase"`), réexporté dans `lib.rs`.
  `Agent` gagne `skill_slug: Option<String>` et `skill_notes: String`.
- **Repo** : `crates/store/src/repo/agent_skills.rs` — `list`, `get`,
  `upsert`, `delete`, `used_by` (agents référençant un slug). Adapter
  `repo::agents::{get,list,upsert}` aux deux nouvelles colonnes.
- **Seed** : `ensure_builtin_agent_skills(db)` dans `crates/store/src/seed.rs`,
  sur le modèle de `ensure_builtin_workflows` — insère un skill livré
  **seulement si son slug est absent**, pour ne jamais écraser une
  modification de l'utilisateur. Contenu via `include_str!` des fichiers
  de `skills/agents/` (voir `qa.md` et `dev-front.md` comme modèles).
  À appeler au démarrage dans `src-tauri/src/lib.rs`, à côté des autres.

## 2. Injection dans le prompt

Dans `crates/engine/src/agent.rs`, fonction `agent_system_prompt` : après
la ligne « Ton rôle : … », insérer

```
## Ta méthode
<contenu du skill de rôle>

### Spécificités de cet agent
<skill_notes>
```

- Sections omises si vides — pas de titre orphelin.
- **Budget** : 4 000 caractères pour le skill de rôle, 800 pour la
  surcouche, coupés avec la fonction `tail` déjà présente dans ce fichier.
  Le prompt est payé à chaque étape : un skill bavard coûte à chaque appel.
- Un slug qui ne correspond à aucune ligne ne doit **pas** faire échouer la
  tâche : ignorer silencieusement et poursuivre.

## 3. Génération du brouillon

`Engine::draft_agent_skill(role: &str, project_id: &ProjectId) -> anyhow::Result<String>`
dans `crates/engine/src/config.rs`.

- Alias de modèle **`reasoning.high`**, via le registre
  (`self.providers().complete(...)`) — jamais un modèle en dur.
- Sortie : markdown structuré (rôle, périmètre, méthode, limites, format de
  compte rendu), **en français**, sans bloc de code englobant, tronquée à
  4 000 caractères. Une sortie vide est une erreur explicite.
- Le prompt de génération interdit d'inventer des outils ou des droits :
  le skill décrit une méthode, pas des permissions.
- **Aucune écriture en base ici.** La fonction renvoie du texte.

## 4. Exposition

- Commandes dans `src-tauri/src/commands/config.rs`, **enregistrées dans
  `invoke_handler` de `src-tauri/src/lib.rs`** : `list_agent_skills`,
  `save_agent_skill`, `delete_agent_skill`, `draft_agent_skill`.
- `delete_agent_skill` refuse si des agents l'utilisent, en les nommant.
- `pnpm bindings`, puis entrées dans `src/ipc/index.ts` **et cas
  correspondants dans `src/ipc/devMock.ts`** (sinon le harnais casse).
- Interface dans `src/ui/settings/AgentsPanel.tsx` : choix du skill de rôle,
  bouton « Rédiger un brouillon… » qui remplit un champ **modifiable** avant
  enregistrement, et une zone « Spécificités de cet agent » pour les notes.
  Suivre la skill `.claude/skills/commande-ipc`.

## Tests attendus — `crates/engine/tests/`

Sans appel réseau. Le fournisseur simulé `Scripted` de
`tests/intelligence.rs` sert de modèle pour la génération.

1. La migration s'applique et le seed est **idempotent** (deux appels, une
   seule ligne) et n'écrase pas un contenu modifié par l'utilisateur.
2. `save_agent` refuse un `skill_slug` inconnu, avec un message nommant le slug.
3. `delete_agent_skill` refuse quand un agent l'utilise et **nomme** cet agent.
4. Le prompt d'une tâche contient le contenu du skill de rôle puis la
   surcouche ; un skill trop long est **coupé au budget** ; un slug orphelin
   ne fait pas échouer la tâche.
5. `draft_agent_skill` avec un `Scripted` : renvoie le texte nettoyé, et
   échoue explicitement sur une réponse vide. **Aucune écriture en base**
   (le vérifier après l'appel).

## Environnement : ce qui ne marche pas en cloud

- **Ne pas compiler `src-tauri`** (WebKit absent) : vérifier avec
  `cargo test --workspace --exclude atelier`. Les fichiers de
  `src-tauri/src/` doivent quand même être écrits correctement — la
  compilation sera vérifiée en local.
- **Ne pas lancer** `real_claude`, `real_ollama`, `real_memory` : ils sont
  `#[ignore]`, exigent un Claude Code connecté ou Ollama, et `real_claude`
  consomme du quota.
- Vérification visuelle : `pnpm dev` (harnais, données simulées) et
  `pnpm typecheck`. Voir la skill `.claude/skills/verif-interface`.

## Hors périmètre

Historique des versions d'un skill, import/export, partage entre machines,
génération automatique sans validation, suppression du champ `skills`.

## Critères d'acceptation

- `cargo test --workspace --exclude atelier` et `pnpm typecheck` passent.
- Les cinq tests ci-dessus existent et échouent si on retire la règle qu'ils
  protègent.
- Une base existante se met à jour sans perte : agents conservés, colonnes
  ajoutées, skills livrés présents.
- Travailler sur une branche dédiée (`skills-agents`), commits en français,
  **sans pousser sur `main`**.
