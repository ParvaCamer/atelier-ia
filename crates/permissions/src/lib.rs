//! Moteur de politique.
//!
//! Évaluation **pure** : pas d'I/O (hors résolution de chemins), pas de
//! base de données, pas d'état. Les règles arrivent en paramètre, la
//! décision sort en retour. C'est ce qui rend ce composant entièrement
//! testable — et c'est indispensable pour la partie la plus sensible du
//! système.
//!
//! Principe : **fail-closed**. L'absence de règle vaut refus.

pub mod danger;
pub mod paths;

use atelier_domain::{Decision, Grant, Mode, ResourceScope};
use std::path::PathBuf;

/// Ce qu'un agent demande à faire.
#[derive(Debug, Clone)]
pub struct ToolRequest {
    pub tool: String,
    pub resource: RequestedResource,
}

#[derive(Debug, Clone)]
pub enum RequestedResource {
    Path(PathBuf),
    Url { host: String },
    Command { program: String, args: Vec<String> },
    None,
}

pub fn evaluate(grants: &[Grant], req: &ToolRequest) -> Decision {
    // 1. Un refus explicite l'emporte sur tout le reste, toujours.
    if let Some(g) = grants.iter().find(|g| matches(g, req) && g.mode == Mode::Deny) {
        return Decision {
            mode: Mode::Deny,
            reason: format!("refus explicite sur `{}`", g.tool),
            escalated: false,
        };
    }

    // 2. Aucune règle applicable → refus. C'est le cœur du fail-closed :
    //    on n'élargit jamais par omission.
    let applicable: Vec<&Grant> = grants.iter().filter(|g| matches(g, req)).collect();
    if applicable.is_empty() {
        return Decision {
            mode: Mode::Deny,
            reason: format!("aucune autorisation pour `{}` sur cette ressource", req.tool),
            escalated: false,
        };
    }

    // 3. Règles de sécurité globales : elles peuvent durcir une
    //    autorisation, jamais l'assouplir.
    if danger::is_always_ask(&req.tool) {
        return Decision {
            mode: Mode::Ask,
            reason: "opération destructrice : confirmation requise".into(),
            escalated: true,
        };
    }
    if let RequestedResource::Command { program, args } = &req.resource {
        if let Some(esc) = danger::inspect_command(program, args) {
            return Decision {
                mode: Mode::Ask,
                reason: esc.reason,
                escalated: true,
            };
        }
    }

    // 4. Sinon, la règle la plus restrictive parmi celles qui s'appliquent.
    //    `Mode` est ordonné Allow < Ask < Deny, d'où le `max`.
    let mode = applicable.iter().map(|g| g.mode).max().unwrap_or(Mode::Deny);
    Decision {
        mode,
        reason: match mode {
            Mode::Allow => "autorisé par la politique de l'agent".into(),
            Mode::Ask => "la politique de l'agent exige une confirmation".into(),
            Mode::Deny => "refusé par la politique de l'agent".into(),
        },
        escalated: false,
    }
}

/// Une règle s'applique-t-elle à cette demande ?
fn matches(grant: &Grant, req: &ToolRequest) -> bool {
    tool_matches(&grant.tool, &req.tool) && scope_matches(&grant.resource, &req.resource)
}

/// Gère la forme `git.*`, qui couvre `git.commit`, `git.status`, etc.
fn tool_matches(pattern: &str, tool: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => tool.starts_with(prefix),
        None => pattern == tool,
    }
}

fn scope_matches(scope: &ResourceScope, req: &RequestedResource) -> bool {
    match (scope, req) {
        (ResourceScope::Any, _) => true,
        (ResourceScope::PathPrefix { path }, RequestedResource::Path(candidate)) => {
            paths::is_within(std::path::Path::new(path), candidate)
        }
        (ResourceScope::UrlHost { host }, RequestedResource::Url { host: h }) => {
            h == host || h.ends_with(&format!(".{host}"))
        }
        (ResourceScope::Command { program }, RequestedResource::Command { program: p, .. }) => {
            p.rsplit('/').next().unwrap_or(p) == program
        }
        // Une règle sans ressource s'applique aux demandes sans ressource
        // (par exemple un outil purement logique).
        (_, RequestedResource::None) => false,
        _ => false,
    }
}
