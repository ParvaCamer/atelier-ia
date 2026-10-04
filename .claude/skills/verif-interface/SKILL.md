---
name: verif-interface
description: Vérifier visuellement l'interface d'Atelier sans compiler Rust, via le harnais navigateur et son IPC simulé. À utiliser après toute modification de src/ui, src/state, src/world ou styles.css, en particulier dans une session cloud où la vraie application ne peut pas être lancée.
---

# Vérifier l'interface sans l'application

La vraie application (`pnpm tauri dev`) demande macOS et un écran. Pour
tout le reste, le harnais navigateur suffit et démarre en une seconde.

```bash
pnpm dev     # http://localhost:1420
```

Hors de la coquille Tauri, `src/ipc/devMock.ts` prend la main et répond à
chaque commande avec des données factices : projets, agents dans tous les
états, workflows, exécutions, mémoire, planifications. Ce n'est **pas** un
mode démo — il n'est jamais actif dans l'application réelle.

## Méthode

1. Ouvrir la page, puis naviguer **en pilotant le DOM** plutôt qu'en
   cliquant à l'aveugle : les écrans de réglages s'ouvrent par le bouton
   « Réglages », puis `.settings-nav-item`.
2. Mesurer plutôt que regarder quand c'est mesurable : `scrollHeight`
   contre `clientHeight` pour un défilement, nombre de `.wf-edge` pour un
   graphe, texte d'un champ pour une saisie. Une capture d'écran prouve
   mal une régression de mise en page.
3. **Les captures d'écran peuvent être en retard d'une action.** Après une
   interaction, attendre puis reprendre une capture avant de conclure.
4. Tester la fenêtre étroite (~1100 px) : l'en-tête et les tableaux de
   réglages ont déjà débordé par le passé.

## Ce que le harnais ne prouve pas

- Le terminal interactif (xterm sur PTY réel) : absent hors Tauri.
- Le sélecteur de dossier natif.
- Les vraies données du moteur : une interface correcte ici peut encore
  échouer sur une erreur renvoyée par le moteur. Les règles, elles, sont
  couvertes par les tests Rust.

## Pièges de mise en page déjà rencontrés

- Un élément de grille ou de flex sans `min-height: 0` grandit à la taille
  de son contenu : la zone interne ne défile jamais et déborde, rognée.
- `vertexColors: true` sans attribut de couleur rend les objets 3D noirs.
- Le rendu 3D se met en pause quand la page est masquée ou qu'un panneau
  plein écran est ouvert : une scène noire dans un aperçu n'est pas un bug.
