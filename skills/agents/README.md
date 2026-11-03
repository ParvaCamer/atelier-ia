# Skills de rôle

Un fichier par **rôle** d'agent : le métier, partagé par tous les projets —
expertise, méthode, limites, format de compte rendu. Les spécificités d'un
agent précis vont dans sa surcouche (Réglages › Agents), et celles d'un
projet dans le `AGENTS.md` ou le `CLAUDE.md` de son dossier, que le moteur
donne à chacun de ses agents. Aucun de ces trois niveaux ne redit les autres.

La règle complète est dans [CLAUDE.md](../../CLAUDE.md).

## Arborescence

Un dossier par famille de métiers, un fichier par rôle. Le nom du fichier
est l'identifiant du skill (`slug`) : il est unique dans tout l'arbre.

```
pilotage/       chef-de-projet, tech-lead
developpement/  dev-front, dev-back, qa
produit/        designer-produit
contenu/        direction-artistique, motion-designer, redaction
marketing/      marketing, seo
exploitation/   ops, supervision
assistance/     assistant, recherche, automatisation
```

## Format

```markdown
---
roles: Développeur Backend, Développeur back-end, Dev back
---

# Développeur backend

## Rôle
## Méthode
## Limites
## Compte rendu
```

- **`roles`** : les intitulés de rôle d'agent que ce skill couvre, séparés
  par des virgules, comparés sans tenir compte de la casse ni des accents.
  Un agent sans skill dont le rôle figure ici le reçoit au démarrage —
  **une seule fois par skill** : si tu le lui retires ensuite, il ne revient
  pas. Un même intitulé ne peut pas figurer dans deux skills.
- L'en-tête n'est pas injecté dans le prompt ; le titre `# …` donne le nom
  du skill. Seuls les 4 000 derniers caractères sont injectés : rester court.

## Livraison

Les fichiers sont embarqués dans le binaire (`include_str!`,
`crates/store/src/seed.rs`) et insérés en base au démarrage **seulement si
leur slug est absent** : une version modifiée par l'utilisateur n'est jamais
écrasée. Ajouter un rôle livré = ajouter le fichier **et** sa ligne dans
`BUILTIN_AGENT_SKILLS`. Un test échoue si un fichier de cet arbre n'y est
pas déclaré, ou si deux skills revendiquent le même intitulé de rôle.
