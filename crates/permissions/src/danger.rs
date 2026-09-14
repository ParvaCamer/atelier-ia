//! Opérations systématiquement escaladées.
//!
//! Cette liste est indépendante de la politique des agents : même un agent
//! à qui `shell.exec` est largement ouvert devra demander confirmation pour
//! ces actions-là. Une autorisation trop large reste ainsi rattrapable.

/// Binaires qui ne sont jamais exécutés sans accord explicite.
const DANGEROUS_PROGRAMS: &[&str] = &[
    "rm", "rmdir", "sudo", "su", "chmod", "chown", "dd", "mkfs", "diskutil",
    "kill", "killall", "pkill", "shutdown", "reboot", "launchctl", "systemctl",
    "curl", "wget", "ssh", "scp", "rsync", "nc", "docker", "kubectl",
];

/// Sous-commandes `git` aux conséquences non locales ou destructrices.
const DANGEROUS_GIT: &[&str] = &["push", "reset", "clean", "rebase", "checkout", "restore"];

/// Fragments d'arguments trahissant une tentative de contournement :
/// enchaînement de commandes, redirection, substitution, élévation.
const DANGEROUS_FRAGMENTS: &[&str] = &[
    "&&", "||", ";", "|", ">", "<", "$(", "`", "sudo ", "rm -rf",
];

pub struct Escalation {
    pub reason: String,
}

/// Analyse une commande. `None` = rien de particulier à signaler.
pub fn inspect_command(program: &str, args: &[String]) -> Option<Escalation> {
    let base = program.rsplit('/').next().unwrap_or(program).to_lowercase();

    if DANGEROUS_PROGRAMS.contains(&base.as_str()) {
        return Some(Escalation {
            reason: format!("`{base}` peut affecter le système hors du projet"),
        });
    }

    if base == "git" {
        if let Some(sub) = args.first() {
            if DANGEROUS_GIT.contains(&sub.as_str()) {
                return Some(Escalation {
                    reason: format!("`git {sub}` modifie l'historique ou publie du code"),
                });
            }
        }
    }

    // Les paquets installés globalement échappent au périmètre du projet.
    if matches!(base.as_str(), "npm" | "pnpm" | "yarn")
        && args.iter().any(|a| a == "-g" || a == "--global" || a == "publish")
    {
        return Some(Escalation {
            reason: "opération npm globale ou publication".into(),
        });
    }

    for arg in args {
        if let Some(frag) = DANGEROUS_FRAGMENTS.iter().find(|f| arg.contains(**f)) {
            return Some(Escalation {
                reason: format!("l'argument contient `{frag}`, ce qui peut enchaîner des commandes"),
            });
        }
    }

    None
}

/// Une suppression n'est jamais automatique, même dans le périmètre autorisé.
pub fn is_always_ask(tool: &str) -> bool {
    matches!(tool, "fs.delete" | "fs.move")
}
