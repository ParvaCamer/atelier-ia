//! Résolution de chemins pour les contrôles d'accès.
//!
//! La comparaison naïve de chaînes est la faille classique de ce genre de
//! système : `/projets/spotly/../../.ssh` a l'air d'être dans le projet, et
//! `/projets/spotly-secret` commence bien par `/projets/spotly`.
//! Tout passe donc par une normalisation puis une comparaison **par
//! composants**, jamais par préfixe textuel.

use std::path::{Component, Path, PathBuf};

/// Normalise un chemin sans exiger qu'il existe : `canonicalize` échoue sur
/// un fichier à créer, or c'est précisément le cas qu'on doit contrôler.
/// Les liens symboliques de la partie existante sont résolus, le reste est
/// nettoyé logiquement.
pub fn normalize(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };

    // Partie existante la plus longue : elle seule peut être canonicalisée
    // (et donc voir ses liens symboliques résolus).
    let mut existing = absolute.as_path();
    let mut rest: Vec<Component> = Vec::new();
    let resolved = loop {
        match existing.canonicalize() {
            Ok(p) => break p,
            Err(_) => match existing.parent() {
                Some(parent) => {
                    if let Some(name) = existing.file_name() {
                        rest.push(Component::Normal(name.as_ref()));
                    }
                    existing = parent;
                }
                None => break absolute.clone(),
            },
        }
    };

    let mut out = resolved;
    for comp in rest.into_iter().rev() {
        match comp {
            Component::Normal(name) => out.push(name),
            _ => {}
        }
    }
    lexical_clean(&out)
}

/// Supprime `.` et remonte sur `..` sans toucher au disque.
fn lexical_clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Vrai si `candidate` est dans le sous-arbre `root` (ou est `root`).
/// Comparaison par composants : `/a/bc` n'est pas dans `/a/b`.
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    let root = normalize(root);
    let candidate = normalize(candidate);
    let mut r = root.components();
    let mut c = candidate.components();
    loop {
        match (r.next(), c.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(a), Some(b)) if a == b => continue,
            _ => return false,
        }
    }
}
