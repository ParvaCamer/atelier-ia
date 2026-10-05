# Atelier — Architecture

> Centre de contrôle personnel pour workflows pilotés par agents IA,
> représenté comme un petit monde 3D vivant.

Document de référence. Toute décision structurante est ici, avec sa
justification et son coût de sortie.

---

## A. Architecture globale

### Principe fondateur

Deux flux **strictement séparés**, qui ne se croisent jamais :

```
   FLUX DE CONTRÔLE (autorité)              FLUX DE REPRÉSENTATION (projection)
   ────────────────────────────             ──────────────────────────────────

   USER
    │  demande / trigger / cron
    ▼
   ORCHESTRATOR ────── plan ──────►  WORKFLOW (DAG persisté)
    │                                   │
    │                                   ▼
    │                                 TASKS  (queued/running/waiting/…)
    │                                   │
    ▼                                   ▼
   SCHEDULER ──── dispatch ────────►  AGENT RUNTIME
                                        │
                                        │ tool_call
                                        ▼
                                     PERMISSIONS  ──► (deny | ask → human)
                                        │ allow
                                        ▼
                                      TOOLS
                                        │
                                        ▼
                            OS / FS / PTY / GIT / HTTP / LLM

                                        │
                                        │  tout produit des ÉVÉNEMENTS
                                        ▼
                                   ╔════════════╗
                                   ║ EVENT BUS  ║  ← unique source de vérité temps réel
                                   ╚════════════╝
                                     │        │
                            ┌────────┘        └────────┐
                            ▼                          ▼
                      STORE (SQLite)            SNAPSHOT BUILDER
                      persistance                coalescing @ 8 Hz
                      historique                        │
                                                        ▼  IPC
                                              ┌──────────────────────┐
                                              │   FRONTEND (webview) │
                                              ├──────────┬───────────┤
                                              │ 3D WORLD │  HUD/UI   │
                                              │ three.js │  React    │
                                              └──────────┴───────────┘
```

**Règle non négociable** : le monde 3D *lit* l'état, il ne le produit jamais.
Supprimer entièrement le dossier `src/world/` doit laisser une application
100 % fonctionnelle. Inversement, le moteur ne sait pas qu'une 3D existe :
il n'émet jamais de position, d'animation ou de coordonnée.

### Le contrat moteur → 3D

Le moteur dit :

```jsonc
{ "agentId": "…", "status": "WORKING", "activity": "SHELL", "taskId": "…", "progress": 0.72 }
```

Il ne dit **jamais** :

```jsonc
{ "x": 12.0, "z": 4.5, "animation": "walk" }   // ❌ interdit
```

C'est la couche 3D qui traduit `activity: SHELL` → « va au poste terminal de
ta zone, joue l'anim typing ». Conséquence directe : on peut changer
totalement la direction artistique, passer en 2D, ou brancher un dashboard
sans toucher une ligne du moteur.

### Découpage en couches

| Couche | Contenu | Dépendances autorisées |
|---|---|---|
| `domain` | Types, ids, états, erreurs. Zéro I/O. | — |
| `bus` | Event bus in-process | domain |
| `store` | SQLite, migrations, requêtes, mémoire | domain |
| `providers` | Abstraction LLM (Anthropic / OpenAI / Ollama) | domain |
| `permissions` | Moteur de politique, évaluation pure | domain |
| `tools` | fs, shell/PTY, git, http | domain, permissions, bus |
| `engine` | Agent runtime, orchestrateur, scheduler | toutes les précédentes |
| `src-tauri` | **Adaptateur mince** : commandes, events, fenêtre | engine + tauri |
| `src/` | Frontend : 3D, HUD, terminal | IPC typé uniquement |

> **Aucun crate de `crates/` ne dépend de `tauri`.** C'est l'assurance-vie de
> l'architecture : si la webview système se révèle insuffisante pour la 3D dans
> 6 mois, on remplace `src-tauri/` (≈ 600 lignes d'adaptateur) et le moteur ne
> bouge pas. Cette contrainte est vérifiable mécaniquement (`cargo tree`).

---

## B. Stack — comparaison et justification

Priorités données, dans l'ordre : perfs, RAM/CPU/GPU, vraie app desktop,
intégration système, terminal réel, FS, réseau, macOS+Windows,
maintenabilité, qualité 3D.

### Shell desktop

| | **Tauri v2** | **Electron** | **Qt / QML** | **Flutter desktop** | **Godot (app entière)** |
|---|---|---|---|---|---|
| RAM à vide | **~90–150 Mo** | ~250–400 Mo | ~80 Mo | ~100 Mo | ~150 Mo |
| Poids binaire | **~12 Mo** | ~180 Mo | ~40 Mo | ~40 Mo | ~70 Mo |
| Backend concurrent | **Rust/tokio, vrais threads** | Node mono-thread + workers | C++ | Dart isolates | GDScript/C# |
| Terminal réel | portable-pty ✅ | node-pty ✅ | ✅ | ⚠️ FFI | ❌ pénible |
| 3D | webview (WebGL2/WebGPU) | Chromium embarqué (identique partout) | Qt3D (écosystème pauvre) | ❌ immature | ✅ excellent |
| UI dense / texte / terminal | ✅ web | ✅ web | ⚠️ | ⚠️ | ❌ très mauvais |
| Risque principal | **divergence WKWebView / WebView2** | consommation | vélocité de dev | 3D inexistante | UI 2D inexploitable |

**Verdict : Tauri v2 + Rust + TypeScript + Three.js.**

Le raisonnement, pas la mode :

1. **L'app est un moteur, pas une UI.** Elle fait tourner N agents en
   parallèle, N processus PTY, des appels HTTP streamés, une DB. C'est un
   travail de backend concurrent. Node mono-thread + `child_process` y arrive
   mais devient le goulot ; Rust/tokio est fait exactement pour ça. Ce point à
   lui seul disqualifie Electron ici — l'argument n'est pas « Electron est
   lourd », c'est « le cœur de cette app est concurrent ».
2. **La 3D est petite.** Une scène low-poly à ~60 draw calls n'a pas besoin
   d'un Chromium dédié. WebGL2 est solide et homogène sur WKWebView et
   WebView2.
3. **L'UI est dense en texte** (terminal, logs, panneaux). Le web est
   imbattable là-dessus. Godot est éliminé pour la raison inverse de Flutter.
4. **Le vrai risque de Tauri** — deux moteurs de rendu différents — est réel
   et j'en tiens compte : pas de WebGPU au MVP, pas de CSS exotique,
   et un plan de sortie (cf. §A) qui coûte un adaptateur, pas un rewrite.

### Moteur 3D

| | **Three.js** | Babylon.js | PlayCanvas |
|---|---|---|---|
| Poids (tree-shaké) | **~150 Ko** | ~1,5 Mo | ~400 Ko |
| Contrôle bas niveau | ✅ total | ✅ | ⚠️ |
| Batteries incluses | ⚠️ minimal | ✅ physique, inspecteur, GUI | ✅ éditeur |
| Adapté ici | **✅** | surdimensionné | orienté cloud/éditeur |

**Three.js r186, renderer WebGL2.** Babylon apporte physique, inspecteur et
système de particules : aucun n'est nécessaire pour des robots low-poly qui
marchent vers un bureau. On paierait 10× le poids pour du confort.

**Décision explicite : pas de react-three-fiber.** R3F relie le graphe de
scène au cycle de rendu React. Avec 50 agents dont l'état change en continu,
on déclencherait des reconciliations React à 60 Hz — exactement le problème
qu'on veut éviter. React gère le HUD ; Three tourne dans sa propre boucle
`requestAnimationFrame` et lit un store en dehors du rendu React. Les deux
mondes ne communiquent que par sélection (3D → React) et snapshot
(store → 3D).

### Persistance

**SQLite via `sqlx` (async, tokio).** Attendu et correct : local-first, un
seul fichier, transactionnel, FTS5 intégré pour la recherche mémoire.
Pas de serveur, pas de dépendance réseau pour les fonctions de base.
`sqlx` plutôt que `rusqlite` pour l'async natif (le moteur est 100 % async)
et les migrations versionnées intégrées.

---

## C. Structure du projet

```
atelier/
├── Cargo.toml                  # workspace Rust
├── crates/
│   ├── domain/                 # types purs, zéro I/O
│   │   └── src/{ids,project,agent,workflow,task,event,memory}.rs
│   ├── bus/                    # event bus (tokio::broadcast)
│   ├── store/
│   │   ├── migrations/         # *.sql versionnées
│   │   └── src/{db,repo/*,memory}.rs
│   ├── providers/
│   │   └── src/{traits,message,anthropic,openai,ollama,registry}.rs
│   ├── permissions/            # évaluation pure de politique
│   ├── tools/
│   │   └── src/{traits,registry,fs,shell,git,http,pty}.rs
│   ├── engine/
│   │   └── src/{agent/,orchestrator/,scheduler/,snapshot.rs}
│   └── src-tauri/ → à la racine (contrainte Tauri)
├── src-tauri/
│   └── src/{lib,commands/*,events,state}.rs     # adaptateur mince
├── src/
│   ├── ipc/                    # bindings typés générés (ts-rs)
│   ├── state/                  # stores zustand
│   ├── world/                  # Three.js — aucun import React
│   │   ├── WorldRenderer.ts    # boucle rAF, caméra, éclairage
│   │   ├── layout.ts           # zones → coordonnées
│   │   ├── agents/             # instancing, steering, anims
│   │   └── assets/             # géométries procédurales
│   ├── ui/                     # React — aucun import three
│   │   ├── Hud/ AgentPanel/ Terminal/ CommandBar/ Approvals/
│   └── app/                    # composition, layout
└── docs/
```

Règle de lint : `src/world/**` ne peut pas importer `react`, `src/ui/**` ne
peut pas importer `three`. Le pont est `src/state/`.

---

## D. Modèle de données

```
Project 1──n Agent
   │  1──n Zone(1:1 au MVP)
   │  1──n Workflow 1──n WorkflowStep
   │              1──n Run 1──n Task 1──n LogLine
   │                              │ n──n Task (dependencies)
   │                              └─1──n ToolCall 1──0/1 Approval
   └─ 1──n MemoryEntry (scope=project)
Agent ─ 1──n MemoryEntry (scope=agent) ─ 1──n Grant (permissions)
```

Tables principales (SQLite) :

| Table | Rôle | Points clés |
|---|---|---|
| `projects` | zone 3D + racine FS + repo git | `root_path` sert de jail FS |
| `agents` | rôle, compétences, modèle, archétype visuel | `model_ref` = alias, pas un modèle en dur |
| `grants` | permissions (agent × tool × resource → mode) | `deny` gagne toujours |
| `workflows` / `workflow_steps` | templates réutilisables | DAG déclaratif en données → éditeur visuel purement additif |
| `runs` | une exécution d'un workflow | statut, durée, résultat |
| `tasks` | unité de travail | `status`, `progress`, `deps`, `result`, `error` |
| `task_deps` | arêtes du DAG | |
| `tool_calls` | trace de chaque appel d'outil | argument + résultat + décision de permission |
| `approvals` | validations humaines en attente | |
| `log_lines` | stdout/stderr/commandes horodatés | indexé `(agent_id, task_id, ts)` → terminal filtrable |
| `memory_entries` | mémoire structurée | `scope`, `kind`, FTS5 |
| `providers_config` | clés API, endpoints, modèles | clés hors DB (keychain OS) |

**États d'une tâche** (machine à états explicite, transitions validées côté Rust) :

```
QUEUED → RUNNING ⇄ PAUSED
   │        ├──► WAITING        (dépendance ou approbation humaine)
   │        ├──► COMPLETED
   │        └──► FAILED ──► QUEUED (retry)
   └──► CANCELLED
```

**Choix de conception** : les tâches ne stockent pas l'historique complet de
conversation. Elles pointent vers des `memory_entries` et des `tool_calls`.
Garder des transcripts bruts en DB est le piège classique : la base gonfle,
et la reprise de contexte reste mauvaise. Cf. §J.

---

## E. Architecture des agents

Un agent = **configuration**, pas une classe. Rien n'est codé en dur par rôle.

```rust
struct AgentSpec {
    id, project_id, name,
    role: String,              // "Frontend Developer"
    system_prompt: String,     // identité + conventions du projet
    skills: Vec<String>,       // injecté dans le prompt + utilisé par le routage
    tools: Vec<ToolId>,        // ce qu'il PEUT demander
    grants: Vec<Grant>,        // ce qu'il a le DROIT de faire  ← différent
    model_ref: ModelRef,       // "reasoning:high" → résolu à l'exécution
    memory_policy: MemoryPolicy,
    archetype: Archetype,      // uniquement pour la 3D
}
```

La distinction `tools` / `grants` est volontaire : *pouvoir demander* ≠
*avoir le droit*. Un agent peut avoir l'outil `shell` et n'avoir le droit de
l'exécuter que dans `~/Projects/Spotly`.

**Boucle d'exécution** (ReAct, bornée) :

```
  contexte ← ContextPacker(task, agent, project, workflow)   # budget en tokens
  loop (max_iterations, max_tokens, max_wall_time) :
      réponse ← provider.complete(messages, tools)
      si pas de tool_call → terminé, on produit un TaskResult
      pour chaque tool_call :
          décision ← permissions.evaluate(agent, tool, args)
          Deny  → observation = erreur explicite (l'agent apprend et s'adapte)
          Ask   → tâche en WAITING, Approval créée, boucle suspendue
          Allow → exécution, streaming des logs sur le bus
      observations → messages
```

Les trois bornes (itérations / tokens / temps) sont obligatoires. Un agent
sans garde-fou en boucle sur un test qui échoue peut brûler un budget
considérable en quelques minutes.

---

## F. Architecture de l'orchestrateur

Trois responsabilités, séparées parce qu'elles ont trois rythmes différents :

| | Rôle | Fréquence | LLM ? |
|---|---|---|---|
| **Router** | classifier la demande, trouver le projet | par demande | petit modèle (local possible) |
| **Planner** | produire un DAG de tâches | par demande | gros modèle |
| **Scheduler** | exécuter le DAG, gérer deps/concurrence/retry | continu | **jamais** |

Le point important : **le scheduler est déterministe**. Aucun LLM ne décide
« quelle tâche lancer maintenant ». Un LLM planifie *une fois*, produit une
structure de données validée, et un code ordinaire l'exécute. C'est ce qui
rend le système observable, reprenable après un crash, et débuggable.

Deux chemins d'entrée, une seule sortie :

```
  demande en langage naturel ──► Router ──► Planner ──┐
                                                      ├──► Run (DAG) ──► Scheduler
  workflow template + trigger ────── instanciation ───┘
```

Un workflow enregistré saute entièrement la phase LLM de planification : il
est déjà un DAG. C'est ce qui rend « Release Spotly » rejouable, rapide et
gratuit.

Sortie du Planner : JSON strictement validé contre un schéma. Si la
validation échoue → une tentative de réparation, puis échec propre.
Jamais d'exécution d'un plan non validé.

### Tableau de l'orchestrateur (implémenté)

Une file de demandes en langage naturel (table `todos`, migration 0012,
`crates/engine/src/todos.rs`) que l'orchestrateur lit et exécute, une à
la fois, en les confiant à `submit_request` — exactement comme une demande
tapée. Le passage sur le tableau (`process_todos`, toutes les 3 s et dès
qu'une tâche y est posée) clôt les tâches dont le run est fini, examine une
proposition, puis lance la prochaine tâche en file.

| Auteur | Entrée | Chemin |
|---|---|---|
| Utilisateur | `add_todo` (panneau « Tableau », ou clic sur le tableau 3D) | en file → exécutée |
| Orchestrateur | `follow_ups` du plan : travaux distincts repérés en planifiant (≤ 2 par plan) | en file → exécutée |
| Chef de projet (archétype `lead`) | outil interne `tableau.proposer`, pendant une de ses tâches (≤ 3 par tâche) | **proposée** → examinée par l'orchestrateur → en file ou refusée |

Garde-fous, tous dans le moteur et couverts par `tests/intelligence.rs` :

- **Fail-closed** : une proposition qu'aucun accord n'a validée ne
  s'exécute jamais. L'examen est un appel unique à `reasoning.default`,
  réponse fermée `accept | reject` ; s'il échoue, la proposition reste
  proposée, avec la raison, et n'est pas réexaminée en boucle.
  L'utilisateur peut toujours trancher à la place de l'orchestrateur.
- **Seuls les chefs proposent** : l'outil n'est présenté qu'aux agents
  d'archétype `lead`, et `propose_todo` refuse les autres. Il ne touche ni
  au système ni aux fichiers : ce n'est pas un outil du registre.
- **Générations bornées** : une suite prend `depth + 1` ; au-delà de
  `MAX_TODO_DEPTH` (2), plus de suite. Sans cette borne, une suite
  pourrait en appeler une autre indéfiniment.
- Doublons (même texte, même projet, encore ouvert), texte vide ou de plus
  de 500 caractères : refusés avec un message qui dit quoi faire.

Aucun appel de modèle dans le scheduler : l'examen et la planification
sont faits par l'orchestrateur, une fois par tâche, avant le run.

### Fournisseurs de modèles (implémenté)

Le moteur ne demande jamais « Claude » ou « Llama » : il demande un **alias**
(`reasoning.high`, `classify.fast`), résolu par la table `model_routes` en
fournisseur + modèle, avec un **repli explicite** par route.

| Alias | Fournisseur | Usage | Repli |
|---|---|---|---|
| `reasoning.high` | Claude Code (abonnement) | planification, agents exigeants | — |
| `reasoning.default` | Claude Code (abonnement) | agents courants | — |
| `classify.fast` | Ollama `llama3.2` | aiguillage des demandes | `reasoning.default` |
| `summarize.fast` | Ollama `llama3.2` | résumés, mémoire (phase 4) | `reasoning.default` |

**OpenAI** (`crates/providers/src/openai.rs`) est un troisième fournisseur,
désactivé par défaut et sans route : facturé à l'usage, il ne sert que si
l'utilisateur fait pointer un alias dessus. Clé saisie dans Réglages › IA,
stockée en base locale, jamais renvoyée à l'interface. Erreurs typées :
clé absente ou refusée → `Unauthorized` (aucun repli), quota → `RateLimited`
(aucun repli), 5xx ou injoignable → `Unavailable` (repli de la route).

**Claude Code CLI comme cerveau sans mains.** Appelé en `claude -p` avec
`--tools ""` : il ne dispose d'aucun outil et n'exécute rien. Il renvoie
une décision JSON validée par schéma ; l'action passe par la porte de
permissions d'Atelier. Lui laisser Bash ou Edit contournerait validations
humaines et audit. Également : `--no-session-persistence` (rien dans
l'historique de l'utilisateur), `--setting-sources ""`, dossier de travail
neutre (aucun hook ni CLAUDE.md de projet chargé), `ANTHROPIC_API_KEY`
retirée de l'environnement (sinon facturation API).

**Conditions d'usage.** Une clé API est facturée séparément de l'abonnement.
Utiliser son propre Claude Code connecté est adapté à un **usage personnel** ;
Anthropic n'autorise pas un développeur à proposer la connexion claude.ai
dans un produit distribué. Si Atelier est publié un jour : fournisseur API.

**À surveiller.** La documentation annonce que `--bare` (qui ignore la
connexion par abonnement) deviendra le défaut de `-p`. Le fournisseur
n'utilise pas ce mode, mais le changement devra être suivi.

**Protocole.** Complétion structurée plutôt qu'appel d'outils natif : c'est
imposé par un CLI sans outils. Un futur fournisseur API pourra exposer
l'appel natif en plus, derrière le même trait.

### Runtime d'agent (implémenté)

Une décision par appel : `{thought, action: tool|finish|fail, tool, args,
summary, progress}`. Le raisonnement (`thought`) est journalisé, l'outil
passe par la porte, l'observation — y compris un refus expliqué — revient au
modèle. Bornes : 20 actions, 20 minutes, 4 erreurs consécutives. Contexte :
identité, projet, demande d'origine, mémoire (conventions + recherche FTS5),
résultats des dépendances, catalogue des outils autorisés. Les anciens
messages sont tronqués pour borner la taille du contexte.

---

## G. Moteur 3D

Budget de performance visé, machine cible = laptop perso :

| Métrique | Budget |
|---|---|
| Draw calls | **< 80** (≈ 77 avec l'usine et 16 agents : la marge est mince) |
| Triangles | **< 150 k** |
| Lumières dynamiques | **1** directionnelle + 1 hémisphérique |
| Shadow maps | **0** au MVP (blob shadows en decal) |
| Post-processing | **0** (FXAA optionnel) |
| Frame budget | **< 6 ms** GPU, < 3 ms CPU |
| Éléments DOM au-dessus de la 3D | **1** popover, jamais un par agent |

Techniques :

- **InstancedMesh par archétype** : 50 agents = 4 draw calls, pas 50.
- **Géométrie procédurale** (pas de fichiers glTF au MVP) : les robots sont
  assemblés à partir de boîtes/cylindres, ~250 tris chacun. Zéro asset à
  charger, zéro texture, style low-poly assumé.
- **Pas d'ombres projetées** : un disque sombre sous chaque agent coûte ~0.
- **Animation sans squelette** : les membres sont des enfants transformés par
  du code. Pas de skinning, pas de GPU skinning, pas de mixer.
- **Interpolation côté client** : le moteur envoie un snapshot à 8 Hz ;
  la 3D interpole à 60 fps. Modèle classique tick serveur + interpolation
  client. L'IPC reste plat quel que soit le nombre d'agents.
- **Boucle suspendue** quand la fenêtre est cachée ou qu'aucun agent ne bouge
  (mode idle → 10 fps).

**Direction artistique (lot 11) : une usine façon Satisfactory.** Chaque
zone est une dalle de fondations quadrillée ; chaque poste est une machine
choisie selon le métier (`machines.ts` : constructeur pour dev/design,
assembleur pour backend/ops, portique de contrôle pour QA/lead, antenne
pour marketing/assistant) dont la pièce mobile s'anime et le panneau
s'allume quand son opérateur y travaille. Les agents sont des pionniers en
combinaison (couleur = métier ; visière, lampe du casque et ombre = état).
Le travail circule sur des convoyeurs suspendus tracés d'après les arêtes
du DAG des runs actifs (`ConveyorLayer.ts`) ; un relais est une caisse qui
file d'une machine à l'autre. Ciel, soleil, brouillard et éclairage
suivent l'heure locale (`environment.ts`) ; la nuit, projecteurs et halos
au sol s'allument. Terrain extraterrestre, roches, flore et falaises au
loin, à peine vallonné (plus de montagnes à l'horizon). `__world.setClock(22)`
(harnais) impose une heure pour juger un éclairage. Environ 87 draw calls et
71 k triangles avec 5 projets, 19 agents et le belvédère.

**Belvédère de l'orchestrateur** (`lookout.ts`, `OrchestratorLayer.ts`) :
une plateforme surélevée (5 m) à charpente orange, plantée dans l'herbe
entre les deux premiers projets de la rangée avant, du côté extérieur —
derrière Spotly et Agency avec la disposition par défaut. Il s'y tient
face au projet qu'il traite ; hologramme et balise prennent la teinte de
son activité. Au repos, toutes les cinq minutes, il fait sa ronde :
descend l'escalier, passe dans l'herbe devant chaque projet (jamais sur
une plateforme : elles sont des obstacles gonflés), s'arrête pour le
regarder, puis remonte. Une décision à prendre le renvoie à son poste.
À ses pieds, le **tableau** recopie la file ouverte du tableau de
l'orchestrateur ; un clic dessus ouvre le panneau correspondant.
`__world.orchestrator.patrolNow()` lance la ronde (harnais) ;
`window.__orchestratorStatus = "idle"` fige l'état simulé.

**Enseignes de zone** : le nom de chaque projet est peint sur un panneau
de tôle à deux poteaux, planté devant le bord avant de sa plateforme, lisible
des deux côtés — plus d'étiquette flottante face à la caméra.

Pipeline de traduction état → visuel (`src/world/palette.ts` pour la
destination, `AgentLayer.ts` pour le mouvement) :

| État moteur | Comportement 3D |
|---|---|
| `IDLE` | debout devant son poste, tourné vers l'allée, respiration |
| `WORKING` + `shell` / `review` | marche jusqu'au bureau, face à l'écran ; frappe au clavier (`shell`) |
| `WORKING` + `files` | marche jusqu'à l'armoire à fichiers |
| `WORKING` + `git` / `network` | marche jusqu'à la baie technique |
| `WORKING` + `thinking` | immobile au bureau, halo qui respire au-dessus de la tête |
| `WAITING` | va au banc d'attente (une place par agent) |
| `NEEDS_APPROVAL` | se tourne vers la caméra, bras levé, « ! » qui rebondit |
| `ERROR` | posture affaissée, tête rouge qui pulse |
| `COMPLETED` | petit saut avec un tour sur soi, bras levés, puis retour au repos |

**Deux vues** (`WorldRenderer.setMode`) : la vue aérienne (orbite, zoom,
déplacement) et la promenade « à pied » (`Visitor.ts`) — l'utilisateur
devient un personnage, caméra à la troisième personne ; touches par
position physique (ZQSD en AZERTY = WASD en QWERTY), Maj pour courir, `V`
pour basculer, Échap pour remonter. Mêmes collisions que les agents ; on
descend des plateformes pour passer d'une zone à l'autre ; s'approcher
d'un agent affiche sa fiche. Purement visuel : le moteur n'en sait rien.

Équipements de décor (`furniture.ts`) : silos, conteneur, caisses,
tuyauterie, générateur, projecteurs, posés par le plan dans les espaces
libres et comptés comme obstacles. Une `InstancedMesh` par pièce.

Déplacements (`src/world/nav.ts`, fonctions pures) : chaque meuble a une
emprise au sol ; un agent dont la ligne droite traverse un meuble passe par
le coin le plus avantageux, deux agents qui se croisent s'écartent, aucun
ne sort de sa plateforme. Le coin retenu tient compte du reste du trajet :
si l'obstacle barre encore la route depuis ce coin, il faudra en passer un
second — sans ce regard en avant, on oscillait devant une longue
plateforme. Au repos (personne ne marche, caméra posée, aucun signal), la
boucle descend à 10 images/s.

Sélection : la sphère englobante d'une `InstancedMesh` est figée par
three.js au premier lancer de rayon ; elle est recalculée à chaque survol,
sans quoi un agent ajouté ensuite ou parti flâner loin du centre (cinquième
projet) devenait impossible à sélectionner.

---

## H. Terminal

Deux besoins différents, donc deux mécanismes — c'est une correction par
rapport à la première version de ce document, qui prévoyait un PTY partout.

| | Commandes d'agents | Terminal de l'utilisateur |
|---|---|---|
| Transport | `tokio::process`, **pipes séparés** | **vrai PTY** (`portable-pty`) + xterm.js |
| stdout / stderr | distincts | fusionnés (nature d'un PTY) |
| Code de sortie | fiable | sans objet |
| Entrée standard | **fermée** | interactive |
| Shell | **aucun** (`programme + args`) | shell de connexion |
| Environnement | reconstruit depuis une liste blanche | celui de l'utilisateur |

Pourquoi pas de PTY pour les agents : un PTY fusionne stdout et stderr et
rend les codes de sortie moins fiables — deux informations dont un agent (et
l'audit) a besoin. Entrée fermée + `CI=1` + `GIT_TERMINAL_PROMPT=0` : une
commande qui pose une question échoue au lieu de bloquer l'agent pour
toujours. Pas de `sh -c` : `;`, `&&`, `|` n'existent pas, et la politique de
permissions raisonne sur le binaire réellement lancé.

Chaque commande d'agent tourne dans **son propre groupe de processus** :

- **Pause** = `SIGSTOP` sur le groupe. Le processus est réellement gelé
  (vérifié par test : état `T` dans `ps`), il ne consomme plus de CPU.
  Sous Windows, la pause reste coopérative (entre deux commandes).
- **Stop** = `SIGKILL` sur le groupe : `npm test` et les `node` qu'il a
  lancés meurent ensemble.

Le terminal d'observation de l'UI n'est pas un composant : c'est une
**vue filtrée** sur un flux de `LogLine { agent_id, task_id, stream, ts, text }`.

```
   global  →  tous les log_lines
   agent   →  WHERE agent_id = ?
   tâche   →  WHERE task_id  = ?
```

Débit borné (lots de 100 ms, plafond par lot), tampon glissant côté UI, le
reste en base avec rétention.

**macOS** : une application lancée depuis le Finder n'hérite pas du `PATH`
du shell (`npm` via Homebrew ou nvm y est introuvable). Le `PATH` est lu une
fois au démarrage depuis le shell de connexion.

---

## I. Système de permissions

Modèle **capability-based, fail-closed**. Rien n'est autorisé par défaut.

```rust
struct Grant {
    agent_id: Option<AgentId>,     // None = s'applique à tout le projet
    project_id: Option<ProjectId>,
    tool: ToolId,                  // fs.read, fs.write, fs.delete, shell.exec, git.*, net.http
    resource: ResourceScope,       // PathPrefix | UrlHost | Any | None
    mode: Mode,                    // Allow | Ask | Deny
}
```

Évaluation, dans cet ordre :

```
1. un Deny correspondant existe          → DENY          (toujours gagnant)
2. aucun Allow/Ask correspondant         → DENY          (fail-closed)
3. l'opération est classée dangereuse    → ASK           (escalade forcée)
4. sinon                                 → mode le plus restrictif qui matche
```

Opérations **toujours escaladées en ASK**, quelle que soit la politique :
suppression récursive, écriture hors de la racine projet, `git push`,
`git reset --hard`, installation globale de paquets, `sudo`, accès réseau
vers un hôte non whitelisté, toute commande contenant une redirection vers un
chemin absolu hors projet.

Défenses concrètes, pas déclaratives :

- Chemins **canonicalisés** (`realpath`) avant comparaison — sinon
  `../../` et les symlinks contournent tout.
- `shell.exec` ne passe **pas** par `sh -c` avec la chaîne brute quand c'est
  évitable : parsing et vérification du binaire + des arguments.
- cwd forcé à la racine projet, env nettoyé (pas de fuite de secrets).
- Chaque décision est écrite dans `tool_calls` : l'audit est rétroactif.

Une escalade `ASK` crée une `Approval`, met la tâche en `WAITING`, et remonte
dans l'UI (et l'agent 3D lève la main). Rien n'attend en silence.

---

## J. Architecture mémoire (implémentée)

Le piège n'est pas d'oublier, c'est de **mémoriser une erreur** : un détail
inventé par un agent deviendrait un « fait » réinjecté dans chaque tâche.

**Portées × natures.** Portée `project` (toute l'équipe) ou `agent` ; natures
`fact`, `convention`, `decision`, `failure`, `artifact`. Les portées `task`
et `workflow` sont couvertes par le contexte d'exécution (résultats des
dépendances), pas par la mémoire persistante.

**Assemblage du contexte** : identité et consignes de l'agent, projet,
demande d'origine, souvenirs de base (triés par importance), souvenirs
rapprochés de la tâche par FTS5, résultats des dépendances, outils autorisés.

**Extraction après chaque tâche** confiée à une IA et après chaque échec
(une commande réussie n'apprend rien). Par le modèle local `summarize.fast`,
**sans repli** : Ollama éteint ⇒ pas de souvenir, jamais un appel caché au
quota de l'abonnement.

Le modèle propose, **du code déterministe dispose**. Chaque garde-fou est issu
d'une mesure réelle sur llama3.2, pas d'une intuition :

| Garde-fou | Problème mesuré |
|---|---|
| Secrets et e-mails rejetés | jeton présent dans une sortie de commande |
| Phrases citant un outil rejetées | « fs.read sur X réussit avec succès » |
| Doublon si recouvrement lexical ou identifiant technique commun | « `testDebugUnitTest` réussit » / « les tests passent avec `testDebugUnitTest` » |
| **Ancrage** : ≥ 60 % des mots significatifs présents dans la trace | « l'émulateur se configure via Android Studio » — culture générale, absente de la trace |
| Autonomie : pas de « ce problème », « cela »… | « pour éviter ce problème, utilisez un émulateur » |
| Importance fixée par la nature | le modèle note tout à 1,0 |

**Recherche par sens (implémentée, lot 6).** Chaque souvenir reçoit un
vecteur calculé par Ollama (`/api/embeddings`, modèle réglable dans
Réglages › IA), rangé avec lui (migration 0009, modèle noté). Au
lancement d'une tâche, le classement FTS5 et le classement par similarité
cosinus (calculée en Rust) sont fusionnés par rang réciproque. Ollama
éteint, modèle absent ou réglage vide : FTS5 seul, à l'identique — jamais
d'appel payant, jamais de repli par le registre. Les souvenirs sans
vecteur sont indexés en fond au démarrage.

Chaque souvenir garde un lien vers la tâche qui l'a produit, et reste
visible, modifiable et supprimable (Réglages › Mémoire). Sa saisie manuelle
passe par les mêmes contrôles (secrets refusés).

---

## Historique (implémenté)

Tout vient de l'audit du moteur, jamais du discours des agents : exécutions
filtrables (projet, état, texte, planifiées), tâches avec agent, durée,
tentatives, résultat ou erreur, et pour chaque tâche la liste des appels
d'outils avec la décision de permission, sa raison, le résultat et le
journal. Relance d'une étape échouée ou de l'exécution entière.

## Coût et consommation (implémentés)

Table `model_usage` (migration 0010) : une ligne par appel à un modèle,
avec ce que le fournisseur annonce (jetons, coût, `served_by`) — aucun
appel supplémentaire pour mesurer. Rattachée à la tâche (décisions
d'agent, extraction de mémoire) et au run (aiguillage et planification,
rattachés après création du run). Agrégée à la lecture : `RunSummary.usage`,
`TaskDetail.usage`, `cost_summary()` pour le cumul du mois (heure locale),
par fournisseur. Coût absent ≠ 0 $ : `None` quand rien n'a été annoncé.

## Planifications (implémentées)

Table unique pour deux cibles : un **workflow** (gratuit à lancer, déjà un
DAG) ou une **demande** en langage naturel (replanifiée à chaque échéance,
donc consomme du quota). Expression cron à 5 champs en **heure locale**,
validée et prévisualisée par le moteur ; l'interface propose des formes
simples (chaque jour, certains jours, toutes les N heures).

- **Ne tourne que quand Atelier est ouvert.** Passage toutes les 30 s.
- Échéance **manquée** (retard > 2 min) : rattrapée **une seule fois** si la
  planification l'autorise, sinon ignorée et consignée.
- **Pas de chevauchement** : si l'exécution précédente tourne encore, l'échéance
  est ignorée et consignée.
- L'échéance suivante est avancée **avant** le lancement : un plantage ne
  provoque jamais de relance en boucle.
- Horloge injectable (`fire_due(now)`) : testé sans attendre un vrai lundi 9 h.
- Supprimer un workflow supprime ses planifications (clé étrangère).

## Surveillance de fichiers (implémentée)

Table `file_watches` (migration 0008) : un workflow, des motifs relatifs
au dossier du projet (`*`, `?`, `**`), un anti-rebond. Scrutation toutes
les 2 s par empreinte (taille + date), sans dépendance ; le premier passage
ne fait que relever l'état. Une rafale = un lancement, après
`debounce_secs` de calme ; exécution précédente en cours ⇒ passage ignoré
et consigné ; projet sans dossier ⇒ refusé à l'enregistrement, suspendu à
l'exécution. Horloge injectable : `poll_watches(now)`. Le run porte
l'origine dans sa demande (« surveillance « X » : src/a.kt (+3) »).

## Éditeur visuel de workflows (implémenté)

Le workflow reste le même objet déclaratif : l'éditeur ne produit rien que
le moteur ne connaissait déjà.

- **Aucune coordonnée stockée.** La disposition se déduit des dépendances :
  une colonne = une vague (plus long chemin depuis une racine), ordre dans la
  colonne par barycentre des parents. Des positions posées à la main
  vieilliraient et mentiraient sur l'ordre réel d'exécution.
- **Liens** : tirer la pastille de sortie d'une étape vers celle qui doit
  l'attendre ; clic sur un lien puis ✕ ou Suppr pour le retirer. Les liens
  impossibles (soi-même, doublon, cycle) sont refusés pendant le geste.
- **Diagnostic du moteur** (`check_workflow`) à chaque modification : toutes
  les erreurs et avertissements rattachés à l'index de l'étape, plus l'agent
  que le lancement choisirait réellement. `save_workflow` applique
  exactement le même diagnostic — aucune règle ne vit seulement dans l'UI.
- Avertissements non bloquants : rôle sans agent actif, agent désactivé,
  étape sans commande ni instruction.
- La clé d'étape suit le titre tant qu'elle n'a pas été choisie ; la renommer
  met à jour les dépendances qui la citent.

## Suivi d'exécution en direct (implémenté)

`RunView` porte l'état de chaque étape (`RunStepView` : tâche, agent,
état, dépendances). C'est le snapshot à 8 Hz qui l'apporte : aucune
commande supplémentaire. Le graphe est celui de l'éditeur
(`WorkflowGraph`) en lecture seule, construit depuis les **tâches** du run
— un plan improvisé par l'orchestrateur n'a pas de workflow enregistré.
`waiting` = attente d'une validation humaine ; une étape qui attend ses
dépendances reste `queued`. Accès : Historique, compteur « workflows » de
l'en-tête, ligne « Workflow » du panneau d'agent.

## Rendus visuels et aperçus en direct (implémentés)

Deux sources, réunies par projet dans la **visionneuse** (bouton « Rendus »)
et sur l'**écran géant** planté à côté du belvédère :

- **Images produites** (`crates/engine/src/renders.rs`, table `renders`,
  migration 0013). À la fin d'une tâche réussie, avant qu'elle soit
  déclarée terminée, le moteur relève dans son dossier de travail les
  images (png, jpg, webp, gif, svg) créées ou modifiées depuis son
  démarrage : slides, maquettes, captures d'une app. Rien à déclarer côté
  agent — une étape sans IA qui génère des slides en produit autant. Ordre
  naturel des fichiers (slide-2 avant slide-10), 24 au plus par tâche,
  15 Mo au plus par image, dépendances et dossiers de compilation ignorés,
  liens symboliques non suivis. L'interface ne lit jamais un chemin :
  `render_data` sert un rendu *enregistré*, en URL `data:`, après avoir
  revérifié qu'il est toujours dans le dossier du projet.
- **Aperçu en direct** : `Project.preview_url`, l'adresse du site du
  projet (serveur local ou en ligne), saisie dans Réglages › Projets et
  validée par le moteur (http/https, sans identifiants). Affiché tel quel
  dans un `iframe` en bac à sable.

L'écran géant (`src/world/ScreenLayer.ts`) montre le projet sur lequel un
run tourne s'il a quelque chose à montrer, sinon celui des derniers rendus :
son site en direct — un vrai document posé en 3D par `CSS3DRenderer`, au
lieu de la dalle, seulement face à la caméra et d'assez près —, sinon le
carrousel de ses images (5 s chacune). Un clic l'ouvre dans la visionneuse.
Une app native (macOS) ne s'intègre pas dans la webview : son rendu passe
par des captures que ses tâches produisent (étape `screencapture`).

---

## Relais entre agents (implémenté)

Quand une tâche terminée en débloque une autre, le scheduler consigne un
`Handoff` par dépendance (table `handoffs`, migration 0006) : tâche
source, tâche cible, les deux agents, extrait du résultat transmis. Il
est publié sur le bus (`DomainEvent::Handoff`), visible dans l'historique
sur la carte de l'étape qui le reçoit, et dans le monde par
`src/world/HandoffLayer.ts` (un arc lumineux, un draw call). Le moteur ne
décrit que qui transmet à qui ; l'animation est une décision de la 3D.

---

## K. Roadmap MVP → V1

**Phase 0 — Socle** ✅
workspace, crates, DB, migrations, bus, IPC typée, app qui démarre.

**Phase 1 — Le monde visible** ✅
rendu 3D, zones, agents instanciés, états visuels, caméra, sélection,
popover ancrée, panneaux HUD. Données depuis SQLite (moteur encore muet).

**Phase 2 — Exécution réelle** ✅
PTY, outil `shell`, `fs`, permissions fail-closed, file d'approbations,
logs streamés et filtrables. *À ce stade, une tâche codée en dur s'exécute
réellement et le monde bouge.*

**Phase 3 — Intelligence** ✅ (mémoire structurée complète en phase 4)
abstraction provider, Anthropic + Ollama, agent runtime (boucle ReAct),
orchestrateur (router → planner → scheduler), workflows persistés.

**Phase 4 — Durabilité** ✅
mémoire structurée + FTS5, extraction post-tâche, historique consultable,
pause/stop/retry complets, triggers planifiés.

**V1** — éditeur visuel de DAG ✅, collaboration inter-agents explicite,
embeddings locaux, plugins d'outils, profils de coût par modèle.

---

## L. Risques techniques

| # | Risque | Gravité | Mitigation |
|---|---|---|---|
| 1 | **WKWebView ≠ WebView2** sur la 3D ou le layout | élevée | WebGL2 uniquement, zéro CSS exotique, budget de draw calls strict, test Windows tôt. Sortie : remplacer l'adaptateur `src-tauri` (§A). |
| 2 | **Agents exécutant du shell** = le vrai danger du projet | **critique** | Fail-closed, jail FS canonicalisé, escalade forcée, audit complet. Traité en Phase 2, pas après. |
| 3 | **Coût / boucles infinies LLM** | élevée | Bornes dures (itérations, tokens, temps) par tâche + budget par run. |
| 4 | **Abstraction multi-provider au plus petit dénominateur** | moyenne | Le format interne modélise le *plus expressif* (blocs typés, tool_use/tool_result) et on **dégrade** vers les autres. L'inverse rend les bons modèles inutilisables. |
| 5 | **Modèles locaux trop faibles pour planifier** | moyenne | Ollama cantonné à classification / résumé / extraction mémoire. Le planner reste sur un gros modèle. Explicite dans la config (`ModelRef` par type de tâche). |
| 6 | Flood d'IPC avec beaucoup d'agents | moyenne | Snapshot coalescé à 8 Hz + batch de logs. Coût constant, pas linéaire. |
| 7 | Croissance de la DB (logs) | faible | Rétention glissante sur `log_lines`, mémoire compressée en entrées typées. |
| 8 | Sur-ingénierie / MVP jamais fini | **réelle** | Phases livrables et vérifiables. Pas de crate créé « au cas où ». |
