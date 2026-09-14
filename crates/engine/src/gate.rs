//! Porte de permissions.
//!
//! **Unique** chemin entre un agent et un outil. Aucun outil ne peut être
//! appelé autrement : la vérification ne peut donc pas être oubliée, elle
//! est structurelle.
//!
//!   demande → ressource → politique → { refus | validation humaine | exécution } → audit

use crate::Engine;
use atelier_domain::*;
use atelier_permissions::{evaluate, RequestedResource, ToolRequest};
use atelier_store::repo;
use atelier_tools::{ToolContext, ToolError, ToolOutput};
use chrono::Utc;
use serde_json::Value;
use std::time::Instant;
use tokio::sync::oneshot;

#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("outil inconnu : {0}")]
    UnknownTool(String),
    #[error("refusé : {0}")]
    Denied(String),
    #[error("refusé par l'utilisateur")]
    Rejected,
    #[error("annulé")]
    Cancelled,
    #[error(transparent)]
    Tool(ToolError),
    #[error("base de données : {0}")]
    Store(#[from] atelier_store::StoreError),
}

impl From<ToolError> for CallError {
    fn from(e: ToolError) -> Self {
        match e {
            ToolError::Cancelled => CallError::Cancelled,
            other => CallError::Tool(other),
        }
    }
}

impl Engine {
    pub async fn call_tool(&self, ctx: &ToolContext, tool_id: &str, args: Value) -> Result<ToolOutput, CallError> {
        let tool = self
            .tools
            .get(tool_id)
            .ok_or_else(|| CallError::UnknownTool(tool_id.into()))?;
        let resource = tool.resource(ctx, &args)?;
        let describe = tool.describe(&args);

        let grants = repo::grants::for_agent(self.db(), &ctx.agent_id, &ctx.project_id).await?;
        let decision = evaluate(&grants, &ToolRequest { tool: tool_id.into(), resource: resource.clone() });

        // Audit écrit AVANT l'exécution : un crash laisse la trace de la tentative.
        let call_id = ToolCallId::new();
        repo::tool_calls::insert(self.db(), &call_id, ctx.task_id.as_ref(), &ctx.agent_id, tool_id, &args, &decision).await?;

        match decision.mode {
            Mode::Deny => {
                ctx.log(LogStream::System, format!("⛔ {describe} — {}", decision.reason));
                self.report_action(&ctx.agent_id, format!("Refusé : {describe}")).await;
                repo::tool_calls::finish(self.db(), &call_id, false, &decision.reason, 0).await?;
                return Err(CallError::Denied(decision.reason));
            }
            Mode::Ask => {
                let granted = self
                    .ask_human(ctx, tool_id, &describe, &args.to_string(), scope_of(&resource), &decision.reason)
                    .await?;
                if !granted {
                    ctx.log(LogStream::System, format!("✋ {describe} — refusé par l'utilisateur"));
                    repo::tool_calls::finish(self.db(), &call_id, false, "refusé par l'utilisateur", 0).await?;
                    return Err(CallError::Rejected);
                }
            }
            Mode::Allow => {}
        }

        self.set_agent_state(&ctx.agent_id, AgentStatus::Working, tool.activity(&args)).await;
        self.report_action(&ctx.agent_id, describe).await;

        let started = Instant::now();
        let result = tool.run(ctx, args).await;
        let ms = started.elapsed().as_millis() as i64;

        let (ok, text) = match &result {
            Ok(o) => (o.ok, truncate(&o.output, 4000)),
            Err(e) => (false, e.to_string()),
        };
        repo::tool_calls::finish(self.db(), &call_id, ok, &text, ms).await?;

        // Retour à « réfléchit » entre deux actions : sans ça l'agent
        // resterait figé devant son terminal après la fin de la commande.
        self.set_agent_state(&ctx.agent_id, AgentStatus::Working, Activity::Thinking).await;
        result.map_err(CallError::from)
    }

    /// Suspend la tâche jusqu'à la décision humaine.
    ///
    /// Aucun délai d'expiration : une opération dangereuse ne doit jamais
    /// passer en force parce que personne n'a répondu à temps.
    pub async fn ask_human(
        &self,
        ctx: &ToolContext,
        tool: &str,
        summary: &str,
        details: &str,
        resource: ResourceScope,
        reason: &str,
    ) -> Result<bool, CallError> {
        let Some(task_id) = ctx.task_id.clone() else {
            return Err(CallError::Denied("validation impossible en dehors d'une tâche".into()));
        };

        let approval = Approval {
            id: ApprovalId::new(),
            agent_id: ctx.agent_id.clone(),
            task_id: task_id.clone(),
            project_id: ctx.project_id.clone(),
            tool: tool.into(),
            summary: summary.into(),
            details: details.into(),
            resource,
            reason: reason.into(),
            created_at: Utc::now(),
            resolved: None,
        };

        // Le canal est enregistré AVANT l'insertion : une réponse instantanée
        // de l'utilisateur ne peut pas arriver avant qu'on l'attende.
        let (tx, rx) = oneshot::channel();
        self.approvals.lock().await.insert(approval.id.clone(), tx);
        repo::approvals::insert(self.db(), &approval).await?;

        let _ = repo::tasks::transition(self.db(), &task_id, TaskStatus::Waiting).await;
        self.set_agent_state(&ctx.agent_id, AgentStatus::NeedsApproval, Activity::None).await;
        self.report_action(&ctx.agent_id, format!("Attend ta validation : {summary}")).await;
        ctx.log(LogStream::System, format!("⏸ validation requise — {summary} ({reason})"));
        self.bus().publish(DomainEvent::ApprovalRequested {
            approval_id: approval.id.clone(),
            agent_id: ctx.agent_id.clone(),
            task_id: task_id.clone(),
            summary: summary.into(),
        });
        let _ = self.refresh_approvals().await;

        let granted = tokio::select! {
            answer = rx => answer.unwrap_or(false),
            _ = ctx.cancel.cancelled() => {
                self.approvals.lock().await.remove(&approval.id);
                let _ = repo::approvals::resolve(self.db(), &approval.id, false).await;
                let _ = self.refresh_approvals().await;
                return Err(CallError::Cancelled);
            }
        };

        let _ = repo::tasks::transition(self.db(), &task_id, TaskStatus::Running).await;
        self.set_agent_state(&ctx.agent_id, AgentStatus::Working, Activity::Thinking).await;
        Ok(granted)
    }

    /// Réponse de l'utilisateur. Idempotente : un double-clic ne relance rien.
    pub async fn resolve_approval(&self, id: &ApprovalId, granted: bool) -> anyhow::Result<bool> {
        let changed = repo::approvals::resolve(self.db(), id, granted).await?;
        if let Some(tx) = self.approvals.lock().await.remove(id) {
            let _ = tx.send(granted);
        }
        if changed {
            self.bus().publish(DomainEvent::ApprovalResolved { approval_id: id.clone(), granted });
        }
        self.refresh_approvals().await?;
        Ok(changed)
    }
}

fn scope_of(r: &RequestedResource) -> ResourceScope {
    match r {
        RequestedResource::Path(p) => ResourceScope::PathPrefix { path: p.to_string_lossy().into() },
        RequestedResource::Url { host } => ResourceScope::UrlHost { host: host.clone() },
        RequestedResource::Command { program, .. } => ResourceScope::Command { program: program.clone() },
        RequestedResource::None => ResourceScope::Any,
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}
