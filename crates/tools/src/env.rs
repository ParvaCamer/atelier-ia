//! Environnement d'exécution des commandes d'agents.
//!
//! Deux problèmes réels traités ici :
//!
//! 1. **Une application macOS lancée depuis le Finder n'hérite pas du PATH
//!    du shell.** `npm`, `pnpm`, `node` installés via Homebrew ou nvm y sont
//!    introuvables. On interroge donc le shell de connexion une fois au
//!    démarrage.
//! 2. **Un agent ne doit hériter d'aucun secret.** L'environnement est
//!    reconstruit à partir d'une liste blanche, pas copié.

use std::collections::HashMap;
use std::process::Command;

pub struct ShellEnv {
    vars: HashMap<String, String>,
}

impl ShellEnv {
    pub fn detect() -> Self {
        let mut vars = HashMap::new();

        let path = login_shell_path().unwrap_or_else(|| {
            let current = std::env::var("PATH").unwrap_or_default();
            format!("{current}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin")
        });
        vars.insert("PATH".into(), path);

        for key in ["HOME", "USER", "LOGNAME", "SHELL", "TMPDIR"] {
            if let Ok(v) = std::env::var(key) {
                vars.insert(key.into(), v);
            }
        }

        // Mode non interactif explicite : sans ça, certains outils ouvrent un
        // prompt et attendent une réponse qui ne viendra jamais.
        for (k, v) in [
            ("LANG", "en_US.UTF-8"),
            ("TERM", "dumb"),
            ("CI", "1"),
            ("NO_COLOR", "1"),
            ("FORCE_COLOR", "0"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_PAGER", "cat"),
            ("PAGER", "cat"),
        ] {
            vars.insert(k.into(), v.into());
        }

        Self { vars }
    }

    pub fn vars(&self) -> &HashMap<String, String> {
        &self.vars
    }

    pub fn path(&self) -> &str {
        self.vars.get("PATH").map(String::as_str).unwrap_or("")
    }
}

fn login_shell_path() -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    // Marqueur : les fichiers de profil peuvent afficher n'importe quoi,
    // on ne garde que ce qui suit notre balise.
    let out = Command::new(shell)
        .args(["-lc", "printf '__ATELIER_PATH__%s' \"$PATH\""])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let path = text.rsplit("__ATELIER_PATH__").next()?.trim();
    (!path.is_empty()).then(|| path.to_string())
}
