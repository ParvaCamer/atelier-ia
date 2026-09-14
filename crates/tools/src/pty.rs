//! Terminal interactif **réel** pour l'utilisateur.
//!
//! Vrai pseudo-terminal (portable-pty) : vim, htop, les couleurs, les
//! invites interactives fonctionnent. Distinct des commandes d'agents
//! (cf. shell.rs), qui ont besoin de flux séparés et de codes de sortie
//! fiables plutôt que d'interactivité.

use atelier_domain::PtyId;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

type OnOutput = Box<dyn Fn(String) + Send + 'static>;
type OnExit = Box<dyn FnOnce() + Send + 'static>;

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default, Clone)]
pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<PtyId, Session>>>,
}

impl PtyManager {
    pub fn open(
        &self,
        cols: u16,
        rows: u16,
        cwd: Option<&Path>,
        on_output: OnOutput,
        on_exit: OnExit,
    ) -> anyhow::Result<PtyId> {
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| {
            if cfg!(windows) { "powershell.exe".into() } else { "/bin/zsh".into() }
        });
        let mut cmd = CommandBuilder::new(shell);
        if !cfg!(windows) {
            cmd.arg("-l"); // shell de connexion : PATH et alias de l'utilisateur
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if let Some(dir) = cwd.filter(|d| d.is_dir()) {
            cmd.cwd(dir);
        } else if let Ok(home) = std::env::var("HOME") {
            cmd.cwd(home);
        }

        let child = pair.slave.spawn_command(cmd)?;
        // Le côté esclave doit être fermé ici : sinon la lecture ne voit
        // jamais la fin du flux quand le shell se termine.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let id = PtyId::new();

        // Lecture bloquante sur un thread dédié : portable-pty n'expose pas
        // d'API asynchrone, et un thread par terminal ouvert est négligeable.
        let sessions = self.sessions.clone();
        let reader_id = id.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut carry: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        carry.extend_from_slice(&buf[..n]);
                        let text = take_valid_utf8(&mut carry);
                        if !text.is_empty() {
                            on_output(text);
                        }
                    }
                }
            }
            sessions.lock().unwrap().remove(&reader_id);
            on_exit();
        });

        self.sessions
            .lock()
            .unwrap()
            .insert(id.clone(), Session { master: pair.master, writer, child });
        Ok(id)
    }

    pub fn write(&self, id: &PtyId, data: &str) -> anyhow::Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        let s = sessions.get_mut(id).ok_or_else(|| anyhow::anyhow!("terminal fermé"))?;
        s.writer.write_all(data.as_bytes())?;
        s.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: &PtyId, cols: u16, rows: u16) -> anyhow::Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let s = sessions.get(id).ok_or_else(|| anyhow::anyhow!("terminal fermé"))?;
        s.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        Ok(())
    }

    pub fn close(&self, id: &PtyId) {
        if let Some(mut s) = self.sessions.lock().unwrap().remove(id) {
            let _ = s.child.kill();
        }
    }
}

/// Un octet UTF-8 multi-octets peut être coupé entre deux lectures.
/// On décode la partie valide et on garde la fin incomplète pour la suite,
/// sinon les accents et les symboles s'affichent en `�` au hasard.
fn take_valid_utf8(buf: &mut Vec<u8>) -> String {
    match std::str::from_utf8(buf) {
        Ok(s) => {
            let out = s.to_string();
            buf.clear();
            out
        }
        Err(e) => {
            let valid = e.valid_up_to();
            // Séquence réellement invalide (pas seulement tronquée) : on
            // remplace plutôt que de bloquer le flux indéfiniment.
            if e.error_len().is_some() {
                let out = String::from_utf8_lossy(buf).to_string();
                buf.clear();
                return out;
            }
            let out = String::from_utf8_lossy(&buf[..valid]).to_string();
            buf.drain(..valid);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::take_valid_utf8;

    #[test]
    fn utf8_coupe_entre_deux_lectures() {
        let bytes = "é".as_bytes(); // 2 octets
        let mut buf = vec![b'a', bytes[0]];
        assert_eq!(take_valid_utf8(&mut buf), "a");
        assert_eq!(buf.len(), 1, "l'octet incomplet est conservé");
        buf.push(bytes[1]);
        assert_eq!(take_valid_utf8(&mut buf), "é");
        assert!(buf.is_empty());
    }
}
