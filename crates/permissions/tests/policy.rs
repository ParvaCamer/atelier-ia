//! La sécurité du système repose sur ces règles : elles sont testées
//! comme telles, y compris sur les tentatives de contournement.

use atelier_domain::{Grant, Mode, ResourceScope};
use atelier_permissions::{evaluate, paths, RequestedResource, ToolRequest};
use std::path::{Path, PathBuf};

fn grant(tool: &str, resource: ResourceScope, mode: Mode) -> Grant {
    Grant { id: tool.into(), agent_id: None, project_id: None, tool: tool.into(), resource, mode }
}

fn read(path: &str) -> ToolRequest {
    ToolRequest { tool: "fs.read".into(), resource: RequestedResource::Path(PathBuf::from(path)) }
}

fn exec(program: &str, args: &[&str]) -> ToolRequest {
    ToolRequest {
        tool: "shell.exec".into(),
        resource: RequestedResource::Command {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
        },
    }
}

#[test]
fn sans_regle_tout_est_refuse() {
    assert_eq!(evaluate(&[], &read("/tmp/x")).mode, Mode::Deny);
}

#[test]
fn le_refus_explicite_gagne_toujours() {
    let grants = vec![
        grant("fs.read", ResourceScope::Any, Mode::Allow),
        grant("fs.read", ResourceScope::Any, Mode::Deny),
    ];
    assert_eq!(evaluate(&grants, &read("/tmp/x")).mode, Mode::Deny);
}

#[test]
fn la_regle_la_plus_restrictive_l_emporte() {
    let grants = vec![
        grant("fs.read", ResourceScope::Any, Mode::Allow),
        grant("fs.read", ResourceScope::Any, Mode::Ask),
    ];
    assert_eq!(evaluate(&grants, &read("/tmp/x")).mode, Mode::Ask);
}

#[test]
fn portee_filesystem_limitee_au_projet() {
    let root = std::env::temp_dir().join("atelier-test-projet");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let grants = vec![grant(
        "fs.read",
        ResourceScope::PathPrefix { path: root.to_string_lossy().into() },
        Mode::Allow,
    )];

    let inside = root.join("src/main.rs");
    assert_eq!(evaluate(&grants, &read(inside.to_str().unwrap())).mode, Mode::Allow);

    // Un dossier voisin dont le nom commence pareil n'est PAS dedans.
    let sibling = format!("{}-secret/creds", root.to_string_lossy());
    assert_eq!(evaluate(&grants, &read(&sibling)).mode, Mode::Deny);

    // Remontée par `..` : neutralisée par la normalisation.
    let escape = root.join("src/../../../../etc/passwd");
    assert_eq!(evaluate(&grants, &read(escape.to_str().unwrap())).mode, Mode::Deny);
}

#[test]
fn les_liens_symboliques_ne_contournent_pas_la_prison() {
    let base = std::env::temp_dir().join("atelier-test-symlink");
    let root = base.join("projet");
    let secret = base.join("secret");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&secret).unwrap();
    std::fs::write(secret.join("cle.txt"), "s3cr3t").unwrap();

    let link = root.join("raccourci");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret, &link).unwrap();

    let grants = vec![grant(
        "fs.read",
        ResourceScope::PathPrefix { path: root.to_string_lossy().into() },
        Mode::Allow,
    )];

    // Le chemin *ressemble* à un chemin du projet ; il pointe ailleurs.
    let through_link = link.join("cle.txt");
    assert_eq!(
        evaluate(&grants, &read(through_link.to_str().unwrap())).mode,
        Mode::Deny,
        "un lien symbolique sortant du projet doit être refusé"
    );
}

#[test]
fn suppression_toujours_soumise_a_confirmation() {
    let grants = vec![grant("fs.delete", ResourceScope::Any, Mode::Allow)];
    let req = ToolRequest {
        tool: "fs.delete".into(),
        resource: RequestedResource::Path(PathBuf::from("/tmp/x")),
    };
    let d = evaluate(&grants, &req);
    assert_eq!(d.mode, Mode::Ask);
    assert!(d.escalated, "l'escalade doit être signalée comme telle");
}

#[test]
fn commandes_dangereuses_escaladees_malgre_une_autorisation_large() {
    let grants = vec![grant("shell.exec", ResourceScope::Any, Mode::Allow)];

    assert_eq!(evaluate(&grants, &exec("npm", &["test"])).mode, Mode::Allow);
    assert_eq!(evaluate(&grants, &exec("rm", &["-rf", "/"])).mode, Mode::Ask);
    assert_eq!(evaluate(&grants, &exec("sudo", &["ls"])).mode, Mode::Ask);
    assert_eq!(evaluate(&grants, &exec("git", &["push"])).mode, Mode::Ask);
    assert_eq!(evaluate(&grants, &exec("git", &["status"])).mode, Mode::Allow);
    assert_eq!(evaluate(&grants, &exec("npm", &["install", "-g", "x"])).mode, Mode::Ask);

    // Enchaînement dissimulé dans un argument.
    assert_eq!(evaluate(&grants, &exec("echo", &["ok; rm -rf /"])).mode, Mode::Ask);
    // Chemin absolu vers un binaire dangereux.
    assert_eq!(evaluate(&grants, &exec("/bin/rm", &["x"])).mode, Mode::Ask);
}

#[test]
fn autorisation_par_binaire() {
    let grants = vec![
        grant("shell.exec", ResourceScope::Command { program: "npm".into() }, Mode::Allow),
    ];
    assert_eq!(evaluate(&grants, &exec("npm", &["test"])).mode, Mode::Allow);
    // `node` n'a pas été autorisé : refus par défaut.
    assert_eq!(evaluate(&grants, &exec("node", &["x.js"])).mode, Mode::Deny);
}

#[test]
fn motif_d_outil_avec_etoile() {
    let grants = vec![grant("git.*", ResourceScope::Any, Mode::Allow)];
    let req = |t: &str| ToolRequest { tool: t.into(), resource: RequestedResource::Path("/tmp".into()) };
    assert_eq!(evaluate(&grants, &req("git.status")).mode, Mode::Allow);
    assert_eq!(evaluate(&grants, &req("fs.write")).mode, Mode::Deny);
}

#[test]
fn normalisation_de_chemin() {
    let tmp = std::env::temp_dir();
    assert!(paths::is_within(&tmp, &tmp.join("a/b/c")));
    assert!(!paths::is_within(Path::new("/a/b"), Path::new("/a/bc")));
    assert!(!paths::is_within(&tmp, Path::new("/etc/passwd")));
}
