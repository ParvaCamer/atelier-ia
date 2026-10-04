---
name: commande-ipc
description: Ajouter une fonctionnalité de bout en bout dans Atelier (type du domaine → moteur → commande Tauri → IPC typée → mock → interface). À utiliser dès qu'une donnée nouvelle doit circuler entre le moteur Rust et l'interface React, ou qu'une commande Tauri est ajoutée ou modifiée.
---

# Ajouter une fonctionnalité de bout en bout

Sept couches, dans cet ordre. En sauter une casse silencieusement soit le
harnais navigateur, soit la compilation du frontend.

## 1. Type — `crates/domain/src/<module>.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MonType { pub mon_champ: String }
```

- `u64` / `i64` deviennent `bigint` en TS : annoter `#[ts(type = "number")]`.
- Pour une énumération à champs : `#[serde(tag = "kind", rename_all = "camelCase")]`.
- Réexporter le type dans `crates/domain/src/lib.rs`.

## 2. Persistance (si la donnée survit au redémarrage)

- Migration **numérotée** dans `crates/store/migrations/` — jamais modifier
  une migration déjà appliquée, en ajouter une nouvelle.
- Accès en base dans `crates/store/src/repo/`. sqlx refuse le SQL construit
  dynamiquement : du SQL constant, et `json_each` pour les listes.

## 3. Logique et validation — `crates/engine/src/`

Toute règle métier vit ici, dans un `impl Engine`. Les réglages sont dans
`config.rs`. L'interface ne valide rien : elle affiche le message d'erreur
du moteur, qui doit nommer l'objet fautif et dire quoi faire.

## 4. Test moteur — `crates/engine/tests/<phase>.rs`

Le test passe par le moteur, pas par les repositories. Vérifier aussi les
refus, pas seulement le cas qui marche.

## 5. Commande Tauri

- Fonction `#[tauri::command]` dans `src-tauri/src/commands/*.rs` : elle
  délègue au moteur et traduit l'erreur (`.map_err(err)`), rien d'autre.
- **L'enregistrer dans `invoke_handler` de `src-tauri/src/lib.rs`** —
  l'oubli ne casse qu'à l'exécution.

## 6. Pont typé

```bash
pnpm bindings     # régénère src/ipc/generated (types commités)
```

Puis une entrée dans `src/ipc/index.ts` (seul endroit autorisé à appeler
`invoke`), et **un cas dans `src/ipc/devMock.ts`** : sans lui, le harnais
navigateur plante sur cette commande.

## 7. Interface

`src/state/` pour l'état, `src/ui/` pour le rendu. `src/world/` n'importe
jamais React, `src/ui/` n'importe jamais three.

## Vérifier

```bash
cargo test -p atelier-engine     # sur Linux ; en local : cargo test --workspace
pnpm typecheck
```

Puis le harnais navigateur (skill `verif-interface`). Ne jamais lancer
`cargo` pendant que `pnpm tauri dev` tourne.
