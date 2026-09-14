//! Outils filesystem. Chemins relatifs à la racine du projet ; le
//! périmètre est vérifié par le moteur de permissions avant l'appel.

use crate::{arg_str, tail, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use atelier_domain::{Activity, LogStream};
use atelier_permissions::RequestedResource;
use serde_json::{json, Value};

const MAX_READ: u64 = 256_000;
const MAX_LIST: usize = 500;

fn path_schema(extra: Value) -> Value {
    let mut props = json!({ "path": { "type": "string", "description": "relatif au projet" } });
    if let (Some(p), Some(e)) = (props.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    let mut required = vec!["path".to_string()];
    required.extend(extra.as_object().into_iter().flat_map(|o| o.keys().cloned()));
    json!({ "type": "object", "properties": props, "required": required })
}

fn path_resource(ctx: &ToolContext, args: &Value) -> Result<RequestedResource, ToolError> {
    Ok(RequestedResource::Path(ctx.resolve(arg_str(args, "path")?)?))
}

fn short(args: &Value) -> &str {
    args.get("path").and_then(Value::as_str).unwrap_or("?")
}

pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn id(&self) -> &'static str { "fs.read" }
    fn description(&self) -> &'static str { "Lit un fichier texte du projet." }
    fn schema(&self) -> Value { path_schema(json!({})) }
    fn activity(&self, _: &Value) -> Activity { Activity::Files }
    fn resource(&self, ctx: &ToolContext, args: &Value) -> Result<RequestedResource, ToolError> {
        path_resource(ctx, args)
    }
    fn describe(&self, args: &Value) -> String { format!("Lecture de {}", short(args)) }

    async fn run(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let path = ctx.resolve(arg_str(&args, "path")?)?;
        let meta = tokio::fs::metadata(&path).await?;
        if meta.len() > MAX_READ {
            return Err(ToolError::InvalidArgs(format!(
                "fichier trop volumineux ({} octets, maximum {MAX_READ})", meta.len()
            )));
        }
        let bytes = tokio::fs::read(&path).await?;
        let text = String::from_utf8_lossy(&bytes).to_string();
        ctx.log(LogStream::System, format!("lu {} ({} octets)", short(&args), bytes.len()));
        Ok(ToolOutput { ok: true, output: text, exit_code: None })
    }
}

pub struct ListDir;

#[async_trait]
impl Tool for ListDir {
    fn id(&self) -> &'static str { "fs.list" }
    fn description(&self) -> &'static str { "Liste le contenu d'un dossier du projet." }
    fn schema(&self) -> Value { path_schema(json!({})) }
    fn activity(&self, _: &Value) -> Activity { Activity::Files }
    fn resource(&self, ctx: &ToolContext, args: &Value) -> Result<RequestedResource, ToolError> {
        path_resource(ctx, args)
    }
    fn describe(&self, args: &Value) -> String { format!("Exploration de {}", short(args)) }

    async fn run(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let path = ctx.resolve(arg_str(&args, "path")?)?;
        let mut entries = tokio::fs::read_dir(&path).await?;
        let mut names = Vec::new();
        while let Some(e) = entries.next_entry().await? {
            let suffix = if e.file_type().await.map(|t| t.is_dir()).unwrap_or(false) { "/" } else { "" };
            names.push(format!("{}{suffix}", e.file_name().to_string_lossy()));
            if names.len() >= MAX_LIST {
                names.push("… (liste tronquée)".into());
                break;
            }
        }
        names.sort();
        // Journalisé comme les lectures : sans cette ligne, on voit l'agent
        // décider d'explorer sans voir ce qu'il a trouvé.
        ctx.log(LogStream::System, format!("listé {} ({} entrées)", short(&args), names.len()));
        Ok(ToolOutput { ok: true, output: names.join("\n"), exit_code: None })
    }
}

pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn id(&self) -> &'static str { "fs.write" }
    fn description(&self) -> &'static str { "Écrit (ou remplace) un fichier texte du projet." }
    fn schema(&self) -> Value { path_schema(json!({ "content": { "type": "string" } })) }
    fn activity(&self, _: &Value) -> Activity { Activity::Files }
    fn resource(&self, ctx: &ToolContext, args: &Value) -> Result<RequestedResource, ToolError> {
        path_resource(ctx, args)
    }
    fn describe(&self, args: &Value) -> String { format!("Modification de {}", short(args)) }

    async fn run(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let path = ctx.resolve(arg_str(&args, "path")?)?;
        let content = arg_str(&args, "content")?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // Écriture atomique : un crash au milieu ne laisse jamais un fichier
        // à moitié écrit dans le projet.
        let tmp = path.with_extension("atelier-tmp");
        tokio::fs::write(&tmp, content).await?;
        tokio::fs::rename(&tmp, &path).await?;
        ctx.log(LogStream::System, format!("écrit {} ({} octets)", short(&args), content.len()));
        Ok(ToolOutput { ok: true, output: tail(&format!("{} octets écrits", content.len()), 200), exit_code: None })
    }
}

pub struct DeleteFile;

#[async_trait]
impl Tool for DeleteFile {
    fn id(&self) -> &'static str { "fs.delete" }
    fn description(&self) -> &'static str { "Supprime un fichier du projet (jamais un dossier)." }
    fn schema(&self) -> Value { path_schema(json!({})) }
    fn activity(&self, _: &Value) -> Activity { Activity::Files }
    fn resource(&self, ctx: &ToolContext, args: &Value) -> Result<RequestedResource, ToolError> {
        path_resource(ctx, args)
    }
    fn describe(&self, args: &Value) -> String { format!("Suppression de {}", short(args)) }

    async fn run(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError> {
        let path = ctx.resolve(arg_str(&args, "path")?)?;
        // Pas de suppression récursive, même approuvée : un dossier se vide
        // fichier par fichier, chaque suppression restant visible et validée.
        if tokio::fs::metadata(&path).await?.is_dir() {
            return Err(ToolError::InvalidArgs("suppression de dossier non prise en charge".into()));
        }
        tokio::fs::remove_file(&path).await?;
        ctx.log(LogStream::System, format!("supprimé {}", short(&args)));
        Ok(ToolOutput { ok: true, output: "supprimé".into(), exit_code: None })
    }
}
