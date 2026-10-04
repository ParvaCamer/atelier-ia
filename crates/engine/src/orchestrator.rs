//! Orchestrateur : demande en langage naturel → run exécutable.
//!
//!   aiguillage (quel projet ?) → planification (quel DAG ?) → validation → run
//!
//! Le modèle intervient **une fois** pour planifier ; le plan est validé
//! par du code (agents existants, commandes lisibles, DAG sans cycle) avant
//! toute exécution. Un plan invalide a droit à une correction, puis échoue
//! proprement — jamais d'exécution d'un plan non validé.

use crate::launch::topological_order;
use crate::Engine;
use atelier_domain::*;
use atelier_providers::{CompletionRequest, Message};
use atelier_store::repo;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

pub const MAX_PLAN_STEPS: usize = 10;
const ROUTER_MIN_CONFIDENCE: f32 = 0.6;

/// Requête d'aiguillage. Publique pour que le test réel mesure exactement
/// la consigne utilisée en production, pas une copie qui divergerait.
///
/// Trois choix issus d'une mesure réelle sur llama3.2 (1 bonne réponse sur 4
/// avec la première version) :
///   * `reason` est demandé AVANT `project` : le modèle raisonne puis tranche ;
///   * `project` est une énumération fermée des noms réels : ni nom inventé,
///     ni variante orthographique ;
///   * le catalogue inclut rôles et compétences des équipes — « Projets clients
///     de l'agence » ne dit pas qu'Agency fait du SEO, son agent SEO le dit.
pub fn router_request(projects: &[Project], agents: &[Agent], text: &str) -> CompletionRequest {
    let catalogue: Vec<String> = projects
        .iter()
        .map(|p| {
            let team: Vec<String> = agents
                .iter()
                .filter(|a| a.project_id == p.id)
                .map(|a| format!("{} ({})", a.role, a.skills.join(", ")))
                .collect();
            format!(
                "- {} : {}. Équipe : {}",
                p.name,
                p.description,
                if team.is_empty() { "aucune".to_string() } else { team.join(" ; ") }
            )
        })
        .collect();
    let mut names: Vec<Value> = projects.iter().map(|p| Value::String(p.name.clone())).collect();
    names.push(Value::Null);

    CompletionRequest {
        system: format!(
            "Tu aiguilles une demande vers l'un des projets suivants, d'après leur description \
             et les compétences de leur équipe :\n{}\n\n\
             Explique d'abord en une phrase ton raisonnement dans `reason`, puis donne le nom exact \
             du projet le plus probable dans `project`. Choisis toujours le projet le plus probable ; \
             `null` uniquement si la demande n'a aucun rapport avec aucun des projets.",
            catalogue.join("\n")
        ),
        messages: vec![Message::user(text)],
        schema: Some(json!({
            "type": "object",
            "properties": {
                "reason": { "type": "string" },
                "project": { "type": ["string", "null"], "enum": names },
                "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
            },
            "required": ["reason", "project", "confidence"]
        })),
        max_tokens: 300,
        temperature: 0.0,
    }
}

#[derive(Deserialize)]
struct RouterAnswer {
    project: Option<String>,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize, Clone)]
struct PlanStep {
    key: String,
    title: String,
    #[serde(default)]
    instruction: String,
    agent: String,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    requires_approval: bool,
}

#[derive(Deserialize)]
struct Plan {
    decision: String,
    #[serde(default)]
    workflow_id: Option<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    reasoning: String,
    #[serde(default)]
    steps: Vec<PlanStep>,
}

enum Resolved {
    Workflow(WorkflowId),
    Steps { title: String, steps: Vec<WorkflowStep> },
}

fn plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "decision": { "type": "string", "enum": ["reuse_workflow", "plan"] },
            "workflow_id": { "type": ["string", "null"] },
            "title": { "type": "string" },
            "reasoning": { "type": "string", "description": "Une ou deux phrases justifiant le plan." },
            "steps": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string" },
                        "title": { "type": "string" },
                        "instruction": { "type": "string" },
                        "agent": { "type": "string" },
                        "depends_on": { "type": "array", "items": { "type": "string" } },
                        "commands": { "type": "array", "items": { "type": "string" } },
                        "requires_approval": { "type": "boolean" }
                    },
                    "required": ["key", "title", "instruction", "agent", "depends_on"]
                }
            }
        },
        "required": ["decision", "title", "reasoning", "steps"]
    })
}

impl Engine {
    pub async fn submit_request(&self, text: &str, project_hint: Option<&ProjectId>) -> anyhow::Result<RunId> {
        let out = self.plan_request(text, project_hint).await;
        // Un échec laisse l'orchestrateur au repos : sans ça, il resterait
        // figé « en planification » jusqu'à la demande suivante.
        if out.is_err() {
            self.set_orchestrator(OrchestratorStatus::Idle, None, None).await;
        }
        out
    }

    async fn plan_request(&self, text: &str, project_hint: Option<&ProjectId>) -> anyhow::Result<RunId> {
        let text = text.trim();
        if text.is_empty() {
            anyhow::bail!("demande vide");
        }
        let cancel = CancellationToken::new();
        self.orchestrator_log(None, format!("◆ demande : « {text} »"));
        self.set_orchestrator(OrchestratorStatus::Routing, None, Some(format!("« {text} »"))).await;
        // Appels faits avant que le run existe : rattachés à lui une fois créé.
        let mut spent: Vec<String> = Vec::new();

        let project = self.route_request(text, project_hint, &cancel, &mut spent).await?;
        self.orchestrator_log(Some(&project.id), format!("◆ projet retenu : {}", project.name));
        self.set_orchestrator(
            OrchestratorStatus::Planning,
            Some(project.id.clone()),
            Some(format!("planifie pour {}", project.name)),
        )
        .await;

        let agents: Vec<Agent> = repo::agents::list(self.db())
            .await?
            .into_iter()
            .filter(|a| a.project_id == project.id && a.enabled)
            .collect();
        if agents.is_empty() {
            anyhow::bail!("le projet {} n'a aucun agent actif", project.name);
        }
        let workflows: Vec<Workflow> = repo::workflows::list(self.db())
            .await?
            .into_iter()
            .filter(|w| w.project_id == project.id && w.enabled)
            .collect();

        let system = planner_prompt(&project, &agents, &workflows);
        let mut messages = vec![Message::user(format!("Demande : {text}"))];
        let mut last_error = String::new();

        for attempt in 0..2 {
            self.orchestrator_log(
                Some(&project.id),
                if attempt == 0 { "◆ planification…" } else { "◆ plan invalide, correction…" },
            );
            let completion = self
                .providers()
                .complete(
                    "reasoning.high",
                    CompletionRequest {
                        system: system.clone(),
                        messages: messages.clone(),
                        schema: Some(plan_schema()),
                        max_tokens: 8_000,
                        temperature: 0.2,
                    },
                    &cancel,
                )
                .await
                .map_err(|e| anyhow::anyhow!("orchestrateur : {e}"))?;
            spent.extend(self.record_usage(None, None, "planning", &completion).await);
            let raw = completion
                .json
                .clone()
                .ok_or_else(|| anyhow::anyhow!("l'orchestrateur n'a pas produit de plan structuré"))?;

            let validated = serde_json::from_value::<Plan>(raw.clone())
                .map_err(|e| format!("structure incorrecte : {e}"))
                .and_then(|plan| validate(plan, text, &agents, &workflows));

            match validated {
                Ok((resolved, reasoning)) => {
                    if !reasoning.is_empty() {
                        self.orchestrator_log(Some(&project.id), format!("◆ {reasoning}"));
                    }
                    let run = match resolved {
                        Resolved::Workflow(id) => {
                            self.orchestrator_log(Some(&project.id), "◆ workflow enregistré réutilisé");
                            self.launch_workflow_with(&id, Some(text)).await?
                        }
                        Resolved::Steps { title, steps } => {
                            let outline: Vec<String> = steps.iter().map(|s| s.title.clone()).collect();
                            self.orchestrator_log(
                                Some(&project.id),
                                format!("◆ plan : {} étape(s) — {}", steps.len(), outline.join(" · ")),
                            );
                            self.create_run(&project.id, &title, Some(text), None, &steps).await?
                        }
                    };
                    repo::usage::attach_to_run(self.db(), &spent, &run).await?;
                    self.set_orchestrator(
                        OrchestratorStatus::Supervising,
                        Some(project.id.clone()),
                        Some(format!("suit « {} »", text)),
                    )
                    .await;
                    return Ok(run);
                }
                Err(e) => {
                    self.orchestrator_log(Some(&project.id), format!("◆ plan rejeté : {e}"));
                    last_error = e.clone();
                    messages.push(Message::assistant(raw.to_string()));
                    messages.push(Message::user(format!("Ce plan est invalide : {e}. Corrige-le en respectant les règles.")));
                }
            }
        }
        anyhow::bail!("plan invalide après correction : {last_error}")
    }

    async fn route_request(&self, text: &str, hint: Option<&ProjectId>, cancel: &CancellationToken, spent: &mut Vec<String>) -> anyhow::Result<Project> {
        let projects = repo::projects::list(self.db()).await?;
        if let Some(id) = hint {
            return projects.into_iter().find(|p| &p.id == id).ok_or_else(|| anyhow::anyhow!("projet introuvable"));
        }
        if projects.len() == 1 {
            return Ok(projects.into_iter().next().expect("un projet"));
        }
        // Projet nommé explicitement : inutile de consulter un modèle.
        let lower = text.to_lowercase();
        let named: Vec<&Project> = projects.iter().filter(|p| lower.contains(&p.name.to_lowercase())).collect();
        if let [only] = named.as_slice() {
            return Ok((*only).clone());
        }

        let agents = repo::agents::list(self.db()).await?;
        let request = router_request(&projects, &agents, text);

        // D'abord le modèle léger (local, gratuit)…
        if let Some(project) = self.ask_router("classify.fast", request.clone(), &projects, cancel, spent).await? {
            return Ok(project);
        }
        // … puis, seulement s'il hésite, le modèle de raisonnement. On ne paie
        // le quota que pour les demandes réellement ambiguës. Pas d'escalade
        // si les deux alias pointent déjà vers le même fournisseur.
        let escalate = match (self.providers().route("classify.fast"), self.providers().route("reasoning.default")) {
            (Some(fast), Some(smart)) => fast.provider_id != smart.provider_id,
            _ => false,
        };
        if escalate {
            self.orchestrator_log(None, "◆ aiguillage local incertain — avis du modèle de raisonnement");
            if let Some(project) = self.ask_router("reasoning.default", request, &projects, cancel, spent).await? {
                return Ok(project);
            }
        }
        anyhow::bail!("projet impossible à déterminer — sélectionne un agent du projet ou nomme-le dans ta demande")
    }

    /// `None` = le modèle n'a pas su trancher (aucun projet ou confiance faible).
    async fn ask_router(
        &self,
        model_ref: &str,
        request: CompletionRequest,
        projects: &[Project],
        cancel: &CancellationToken,
        spent: &mut Vec<String>,
    ) -> anyhow::Result<Option<Project>> {
        let completion = self
            .providers()
            .complete(model_ref, request, cancel)
            .await
            .map_err(|e| anyhow::anyhow!("aiguillage : {e}"))?;
        spent.extend(self.record_usage(None, None, "routing", &completion).await);
        let answer: RouterAnswer = serde_json::from_value(completion.json.unwrap_or(Value::Null))
            .map_err(|e| anyhow::anyhow!("aiguillage illisible : {e}"))?;

        let found = answer
            .project
            .as_deref()
            .and_then(|name| projects.iter().find(|p| p.name.eq_ignore_ascii_case(name.trim())))
            .filter(|_| answer.confidence >= ROUTER_MIN_CONFIDENCE);
        if let Some(p) = found {
            self.orchestrator_log(Some(&p.id), format!("◆ aiguillage ({}) : {} — {}", completion.served_by, p.name, answer.reason));
        }
        Ok(found.cloned())
    }

    /// Journal de l'orchestrateur : lignes sans agent, rattachées au projet.
    pub(crate) fn orchestrator_log(&self, project: Option<&ProjectId>, text: impl Into<String>) {
        let mut line = LogLine::system(text);
        line.project_id = project.cloned();
        self.bus().log(line);
    }
}

fn planner_prompt(project: &Project, agents: &[Agent], workflows: &[Workflow]) -> String {
    let team: Vec<String> = agents
        .iter()
        .map(|a| format!("- {} — {} ; compétences : {} ; outils : {}", a.name, a.role, a.skills.join(", "), a.tools.join(", ")))
        .collect();
    let saved: Vec<String> = workflows
        .iter()
        .map(|w| format!("- id={} « {} » — {} ({} étapes)", w.id, w.name, w.description, w.steps.len()))
        .collect();

    format!(
        "Tu es l'orchestrateur d'Atelier. Tu transformes une demande en plan d'exécution pour une équipe \
         d'agents spécialisés. Tu ne réalises rien toi-même.\n\n\
         Projet : {} — {}\n\
         Répertoire local : {}\n\n\
         Agents disponibles (utilise leur nom EXACT) :\n{}\n\n\
         Workflows enregistrés :\n{}\n\n\
         Règles :\n\
         1. Si un workflow enregistré correspond exactement à la demande, réutilise-le \
            (decision = \"reuse_workflow\", workflow_id, steps = []). Sinon decision = \"plan\".\n\
         2. Au plus {MAX_PLAN_STEPS} étapes, le moins possible. Une demande simple tient en une seule étape.\n\
         3. Chaque étape va à l'agent dont le rôle convient le mieux.\n\
         4. `commands` seulement pour une étape purement mécanique dont les commandes sont connues d'avance \
            (ex. [\"npm test\"]). Sinon laisse la liste vide : l'agent décidera lui-même.\n\
         5. Commandes : ni pipe, ni redirection, ni `;`, ni `&&`.\n\
         6. Parallélise les étapes indépendantes via depends_on ; clés courtes en snake_case.\n\
         7. requires_approval = true avant toute étape irréversible ou qui publie quelque chose.\n\
         8. instruction : ce que l'agent doit accomplir et à quoi il saura que c'est terminé.",
        project.name,
        project.description,
        project.root_path.as_deref().unwrap_or("aucun : les agents n'ont pas accès aux fichiers"),
        team.join("\n"),
        if saved.is_empty() { "(aucun)".to_string() } else { saved.join("\n") },
    )
}

fn validate(plan: Plan, request: &str, agents: &[Agent], workflows: &[Workflow]) -> Result<(Resolved, String), String> {
    match plan.decision.as_str() {
        "reuse_workflow" => {
            let id = plan.workflow_id.unwrap_or_default();
            workflows
                .iter()
                .find(|w| w.id.as_str() == id)
                .map(|w| (Resolved::Workflow(w.id.clone()), plan.reasoning.clone()))
                .ok_or_else(|| format!("workflow « {id} » inconnu pour ce projet"))
        }
        "plan" => {
            if plan.steps.is_empty() {
                return Err("aucune étape".into());
            }
            if plan.steps.len() > MAX_PLAN_STEPS {
                return Err(format!("{} étapes, maximum {MAX_PLAN_STEPS}", plan.steps.len()));
            }
            let mut steps = Vec::with_capacity(plan.steps.len());
            for s in plan.steps {
                let agent = agents
                    .iter()
                    .find(|a| a.name.eq_ignore_ascii_case(s.agent.trim()))
                    .ok_or_else(|| {
                        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
                        format!("agent inconnu « {} » (disponibles : {})", s.agent, names.join(", "))
                    })?;
                for c in &s.commands {
                    atelier_tools::shell::parse(c).map_err(|e| format!("commande « {c} » : {e}"))?;
                }
                steps.push(WorkflowStep {
                    // L'orchestrateur ne choisit pas de sous-dossier : les
                    // étapes qu'il improvise travaillent à la racine.
                    cwd: None,
                    key: s.key,
                    title: s.title,
                    instruction: s.instruction,
                    agent_id: Some(agent.id.clone()),
                    role_hint: None,
                    depends_on: s.depends_on,
                    requires_approval: s.requires_approval,
                    commands: s.commands,
                });
            }
            topological_order(&steps).map_err(|e| e.to_string())?;
            let title = if plan.title.trim().is_empty() {
                request.chars().take(60).collect()
            } else {
                plan.title
            };
            Ok((Resolved::Steps { title, steps }, plan.reasoning))
        }
        other => Err(format!("décision inconnue « {other} »")),
    }
}
