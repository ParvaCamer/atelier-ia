//! Exécution de commandes par les agents.
//!
//! Pas de PTY ici, volontairement : un PTY fusionne stdout et stderr et
//! rend les codes de sortie moins fiables. Pas de `sh -c` non plus : la
//! commande est découpée en programme + arguments, ce qui rend
//! l'enchaînement (`;`, `&&`, `|`) impossible et permet au moteur de
//! permissions de raisonner sur le binaire réellement lancé.
//!
//! Le processus est placé dans son propre groupe : arrêter une tâche tue
//! aussi les processus enfants (`npm test` lance `node`, qui lance…).

use crate::{arg_str, tail, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use atelier_domain::{Activity, LogStream, TaskId};
use atelier_permissions::RequestedResource;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

const DEFAULT_TIMEOUT_S: u64 = 600;
const MAX_OUTPUT: usize = 16_000;

/// Processus en cours, par tâche. Sert à suspendre / reprendre / tuer
/// depuis l'interface pendant qu'une commande tourne.
#[derive(Default)]
pub struct ProcessTable {
    inner: Mutex<HashMap<TaskId, u32>>,
}

impl ProcessTable {
    fn register(&self, task: &TaskId, pid: u32) {
        self.inner.lock().unwrap().insert(task.clone(), pid);
    }

    fn unregister(&self, task: &TaskId) {
        self.inner.lock().unwrap().remove(task);
    }

    /// Pause réelle : le groupe de processus est gelé (SIGSTOP), il ne
    /// consomme plus de CPU et reprendra exactement où il en était.
    pub fn suspend(&self, task: &TaskId) -> bool {
        self.signal(task, Signal::Stop)
    }

    pub fn resume(&self, task: &TaskId) -> bool {
        self.signal(task, Signal::Continue)
    }

    pub fn has(&self, task: &TaskId) -> bool {
        self.inner.lock().unwrap().contains_key(task)
    }

    fn signal(&self, task: &TaskId, sig: Signal) -> bool {
        match self.inner.lock().unwrap().get(task) {
            Some(pid) => send_group(*pid, sig),
            None => false,
        }
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Stop,
    Continue,
    Kill,
}

#[cfg(unix)]
fn send_group(pid: u32, sig: Signal) -> bool {
    let s = match sig {
        Signal::Stop => libc::SIGSTOP,
        Signal::Continue => libc::SIGCONT,
        Signal::Kill => libc::SIGKILL,
    };
    // pid négatif = tout le groupe de processus.
    unsafe { libc::kill(-(pid as i32), s) == 0 }
}

#[cfg(not(unix))]
fn send_group(_pid: u32, _sig: Signal) -> bool {
    // Windows : pas de SIGSTOP. La pause y reste coopérative (entre deux
    // commandes) ; l'arrêt passe par `kill_on_drop`.
    false
}

pub struct ShellExec;

pub struct ParsedCommand {
    pub program: String,
    pub args: Vec<String>,
}

pub fn parse(command: &str) -> Result<ParsedCommand, ToolError> {
    let mut parts = shell_words::split(command)
        .map_err(|e| ToolError::InvalidArgs(format!("commande illisible : {e}")))?;
    if parts.is_empty() {
        return Err(ToolError::InvalidArgs("commande vide".into()));
    }
    let program = parts.remove(0);
    Ok(ParsedCommand { program, args: parts })
}

#[async_trait]
impl Tool for ShellExec {
    fn id(&self) -> &'static str {
        "shell.exec"
    }

    fn description(&self) -> &'static str {
        "Exécute une commande dans le répertoire du projet. Pas de shell : \
         ni pipe, ni redirection, ni enchaînement. Entrée standard fermée."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "ex. \"npm test\"" },
                "cwd": { "type": "string", "description": "sous-dossier relatif au projet" },
                "timeout_s": { "type": "integer", "minimum": 1, "maximum": 3600 }
            },
            "required": ["command"]
        })
    }

    fn activity(&self, args: &Value) -> Activity {
        let program = args
            .get("command")
            .and_then(Value::as_str)
            .and_then(|c| parse(c).ok())
            .map(|p| p.program);
        match program.as_deref() {
            Some("git") => Activity::Git,
            Some("curl") | Some("wget") => Activity::Network,
            _ => Activity::Shell,
        }
    }

    fn resource(&self, _ctx: &ToolContext, args: &Value) -> Result<RequestedResource, ToolError> {
        let parsed = parse(arg_str(args, "command")?)?;
        Ok(RequestedResource::Command { program: parsed.program, args: parsed.args })
    }

    fn describe(&self, args: &Value) -> String {
        format!("$ {}", args.get("command").and_then(Value::as_str).unwrap_or("?"))
    }

    async fn run(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let command = arg_str(&args, "command")?.to_string();
        let parsed = parse(&command)?;
        let root = ctx.root()?.to_path_buf();
        let cwd = match args.get("cwd").and_then(Value::as_str) {
            Some(sub) => ctx.resolve(sub)?,
            None => root.clone(),
        };
        // Le sous-dossier doit rester dans le projet : la politique a validé
        // le binaire, pas l'endroit où il s'exécute.
        if !atelier_permissions::paths::is_within(&root, &cwd) {
            return Err(ToolError::InvalidArgs("le répertoire de travail sort du projet".into()));
        }
        let timeout = args
            .get("timeout_s")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_S)
            .clamp(1, 3600);

        ctx.log(LogStream::Command, format!("$ {command}"));

        // `Command::new` cherche le binaire dans le PATH du *processus*
        // Atelier, pas dans celui qu'on transmet à l'enfant : on résout donc
        // nous-mêmes, sinon `npm` reste introuvable hors d'un terminal.
        let program = which(&parsed.program, ctx.env.path())
            .unwrap_or_else(|| std::path::PathBuf::from(&parsed.program));

        let mut cmd = Command::new(&program);
        cmd.args(&parsed.args)
            .current_dir(&cwd)
            .env_clear()
            .envs(ctx.env.vars())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd.spawn().map_err(|e| {
            ctx.log(LogStream::Stderr, format!("impossible de lancer `{}` : {e}", parsed.program));
            ToolError::Io(e)
        })?;
        let pid = child.id();
        if let (Some(pid), Some(task)) = (pid, &ctx.task_id) {
            ctx.processes.register(task, pid);
        }

        let stdout = child.stdout.take().expect("stdout pipé");
        let stderr = child.stderr.take().expect("stderr pipé");

        // Les deux flux sont lus en parallèle : lire l'un puis l'autre peut
        // bloquer le processus si le tampon du second se remplit.
        let out_ctx = ctx.clone();
        let out_task = tokio::spawn(async move { pump(out_ctx, stdout, LogStream::Stdout).await });
        let err_ctx = ctx.clone();
        let err_task = tokio::spawn(async move { pump(err_ctx, stderr, LogStream::Stderr).await });

        let outcome = tokio::select! {
            status = child.wait() => Ok(status?),
            _ = ctx.cancel.cancelled() => Err(ToolError::Cancelled),
            _ = tokio::time::sleep(std::time::Duration::from_secs(timeout)) => Err(ToolError::Timeout(timeout)),
        };

        if outcome.is_err() {
            if let Some(pid) = pid {
                send_group(pid, Signal::Kill);
            }
            let _ = child.kill().await;
        }
        if let Some(task) = &ctx.task_id {
            ctx.processes.unregister(task);
        }

        let mut output = out_task.await.unwrap_or_default();
        let err_text = err_task.await.unwrap_or_default();
        if !err_text.is_empty() {
            output.push_str("\n[stderr]\n");
            output.push_str(&err_text);
        }

        match outcome {
            Ok(status) => {
                let code = status.code();
                let ok = status.success();
                ctx.log(
                    LogStream::System,
                    match code {
                        Some(c) => format!("↳ code de sortie {c}"),
                        None => "↳ interrompu par un signal".into(),
                    },
                );
                Ok(ToolOutput { ok, output: tail(&output, MAX_OUTPUT), exit_code: code })
            }
            Err(e) => {
                ctx.log(LogStream::System, format!("↳ {e}"));
                Err(e)
            }
        }
    }
}

async fn pump<R: tokio::io::AsyncRead + Unpin>(ctx: ToolContext, reader: R, stream: LogStream) -> String {
    let mut lines = BufReader::new(reader).lines();
    let mut collected = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        ctx.log(stream, line.clone());
        // On collecte large puis on tronque à la fin : les erreurs utiles
        // sont presque toujours dans les dernières lignes.
        if collected.len() < MAX_OUTPUT * 8 {
            collected.push_str(&line);
            collected.push('\n');
        }
    }
    collected
}

pub fn which(program: &str, path: &str) -> Option<std::path::PathBuf> {
    if program.contains('/') {
        return None;
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    path.split(sep)
        .map(|dir| std::path::Path::new(dir).join(program))
        .find(|candidate| candidate.is_file())
}
