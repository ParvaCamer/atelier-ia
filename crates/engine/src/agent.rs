//! Runtime d'agent IA.
//!
//! Boucle bornée : le modèle décide **une** action à la fois, au format JSON
//! validé par schéma ; le moteur l'exécute via la porte de permissions et
//! renvoie l'observation. Le modèle n'exécute jamais rien lui-même.
//!
//!   contexte → décision → porte de permissions → outil → observation → …
//!
//! Trois garde-fous obligatoires : nombre d'actions, durée totale, erreurs
//! consécutives. Sans eux, un agent qui bute sur un test peut boucler
//! indéfiniment en consommant le quota.

use crate::scheduler::Failure;
use crate::{CallError, Engine};
use atelier_domain::*;
use atelier_providers::{CompletionRequest, Message, ProviderError};
use atelier_store::repo;
use atelier_tools::ToolContext;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub const MAX_STEPS: usize = 20;
const MAX_CONSECUTIVE_ERRORS: usize = 4;
const MAX_WALL: Duration = Duration::from_secs(20 * 60);
const OBSERVATION_MAX: usize = 6_000;
/// Messages récents gardés intacts ; les plus anciens sont tronqués pour
/// que le contexte ne grossisse pas linéairement avec les étapes.
const KEEP_RECENT: usize = 8;
/// Budgets d'injection du skill de rôle et de la surcouche. Le prompt
/// système est payé à chaque décision : un skill bavard coûte à chaque appel.
pub const SKILL_BUDGET: usize = 4_000;
pub const SKILL_NOTES_BUDGET: usize = 800;
/// Souvenirs injectés au plus, dont ceux rapprochés de la tâche.
const MEMORY_LINES: usize = 20;
const MEMORY_HITS: usize = 6;

#[derive(Debug, Deserialize)]
struct AgentAction {
    #[serde(default)]
    thought: String,
    action: String,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    progress: Option<f32>,
}

pub fn action_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "thought": { "type": "string", "description": "Une phrase : ce que tu fais maintenant et pourquoi." },
            "action": { "type": "string", "enum": ["tool", "finish", "fail"] },
            "tool": { "type": ["string", "null"], "description": "Identifiant de l'outil si action = tool." },
            "args": { "type": ["object", "null"], "description": "Arguments de l'outil, selon son schéma." },
            "summary": { "type": ["string", "null"], "description": "Si finish ou fail : compte rendu factuel." },
            "progress": { "type": ["number", "null"], "minimum": 0, "maximum": 1 }
        },
        "required": ["thought", "action"]
    })
}

impl Engine {
    pub(crate) async fn run_agent(
        &self,
        task: &Task,
        ctx: &ToolContext,
        paused: &mut watch::Receiver<bool>,
    ) -> Result<String, Failure> {
        let fail = Failure::Failed;
        let agent = repo::agents::get(self.db(), &task.agent_id).await.map_err(|e| fail(e.to_string()))?;
        if self.providers().route(&agent.model_ref).is_none() {
            return Err(fail(format!(
                "aucun fournisseur IA configuré pour « {} » : cette étape doit être confiée à un agent IA",
                agent.model_ref
            )));
        }

        let tools: Vec<String> = agent.tools.iter().filter(|t| self.tools.get(t).is_some()).cloned().collect();
        let system = self.agent_system_prompt(&agent, task, &tools).await.map_err(|e| fail(e.to_string()))?;
        let brief = if task.description.trim().is_empty() { "(pas de précision supplémentaire)" } else { task.description.as_str() };
        let mut messages = vec![Message::user(format!("Tâche : {}\n\n{brief}", task.title))];

        let started = Instant::now();
        let mut errors = 0usize;
        let mut progress = 0.0f32;
        let mut cost = 0.0f64;

        for step in 1..=MAX_STEPS {
            wait_if_paused(paused, &ctx.cancel).await?;
            if started.elapsed() > MAX_WALL {
                return Err(fail(format!("durée maximale de {} min dépassée", MAX_WALL.as_secs() / 60)));
            }

            self.set_agent_state(&task.agent_id, AgentStatus::Working, Activity::Thinking).await;
            compact(&mut messages);
            let request = CompletionRequest {
                system: system.clone(),
                messages: messages.clone(),
                schema: Some(action_schema()),
                max_tokens: 4_096,
                temperature: 0.2,
            };
            let completion = match self.providers().complete(&agent.model_ref, request, &ctx.cancel).await {
                Ok(c) => c,
                Err(ProviderError::Cancelled) => return Err(Failure::Cancelled),
                Err(e) => return Err(fail(format!("fournisseur IA : {e}"))),
            };
            cost += completion.usage.cost_usd.unwrap_or(0.0);
            self.record_usage(Some(&task.run_id), Some(&task.id), "agent", &completion).await;

            let raw = completion.json.clone().unwrap_or(Value::Null);
            let action: AgentAction = match serde_json::from_value(raw.clone()) {
                Ok(a) => a,
                Err(e) => {
                    errors += 1;
                    if errors >= MAX_CONSECUTIVE_ERRORS {
                        return Err(fail(format!("réponses du modèle inexploitables : {e}")));
                    }
                    messages.push(Message::assistant(completion.text));
                    messages.push(Message::user(format!("Réponse invalide ({e}). Réponds strictement selon le schéma.")));
                    continue;
                }
            };

            // Le raisonnement est journalisé : on voit ce que l'agent décide,
            // pas seulement ce qu'il exécute.
            if !action.thought.is_empty() {
                ctx.log(LogStream::System, format!("💭 {}", action.thought));
            }
            if let Some(p) = action.progress.filter(|p| *p > progress) {
                // Jamais 100 % avant la fin réelle : c'est le moteur qui conclut.
                progress = p.min(0.95);
                self.set_task_progress(&task.agent_id, &task.id, progress).await;
            }
            messages.push(Message::assistant(raw.to_string()));

            match action.action.as_str() {
                "finish" => {
                    ctx.log(LogStream::System, format!("↳ {step} décision(s) · {}", served_cost(cost, &completion.served_by)));
                    return Ok(action.summary.filter(|s| !s.trim().is_empty()).unwrap_or(action.thought));
                }
                "fail" => {
                    return Err(fail(action.summary.unwrap_or_else(|| "l'agent a abandonné sans explication".into())));
                }
                "tool" => {}
                other => {
                    errors += 1;
                    messages.push(Message::user(format!("Action inconnue « {other} ». Valeurs possibles : tool, finish, fail.")));
                    continue;
                }
            }

            let observation = match action.tool.as_deref() {
                None => {
                    errors += 1;
                    "ERREUR : action « tool » sans nom d'outil.".to_string()
                }
                Some(tool) if !tools.iter().any(|t| t == tool) => {
                    errors += 1;
                    format!("ERREUR : « {tool} » ne fait pas partie de tes outils ({}).", tools.join(", "))
                }
                Some(tool) => match self.call_tool(ctx, tool, action.args.clone().unwrap_or_else(|| json!({}))).await {
                    Ok(out) => {
                        errors = 0;
                        let code = out.exit_code.map(|c| format!(", code {c}")).unwrap_or_default();
                        format!(
                            "Résultat de {tool} ({}{code}) :\n{}",
                            if out.ok { "succès" } else { "échec" },
                            tail(&out.output, OBSERVATION_MAX)
                        )
                    }
                    Err(CallError::Cancelled) => return Err(Failure::Cancelled),
                    Err(CallError::Denied(reason)) => {
                        errors += 1;
                        format!("REFUSÉ par la politique de permissions : {reason}. Insister ne changera rien : adapte-toi ou termine en échec.")
                    }
                    Err(CallError::Rejected) => {
                        errors += 1;
                        "REFUSÉ par l'utilisateur. Ne retente pas cette opération.".to_string()
                    }
                    Err(e) => {
                        errors += 1;
                        format!("ERREUR : {e}")
                    }
                },
            };

            if errors >= MAX_CONSECUTIVE_ERRORS {
                return Err(fail(format!(
                    "{MAX_CONSECUTIVE_ERRORS} erreurs consécutives — dernière : {}",
                    observation.lines().next().unwrap_or_default()
                )));
            }
            messages.push(Message::user(format!("{observation}\n\n(action {step}/{MAX_STEPS})")));
        }

        Err(fail(format!("limite de {MAX_STEPS} actions atteinte sans terminer")))
    }

    async fn agent_system_prompt(&self, agent: &Agent, task: &Task, tools: &[String]) -> anyhow::Result<String> {
        let db = self.db();
        let project = repo::projects::get(db, &task.project_id).await?;
        let run = repo::runs::get(db, &task.run_id).await?;

        let mut out = format!("{}\n\n## Contexte\n", agent.system_prompt);
        out.push_str(&format!("Projet : {} — {}\n", project.name, project.description));
        out.push_str(&format!(
            "Répertoire du projet : {}\n",
            project.root_path.as_deref().unwrap_or("aucun (tu n'as pas accès aux fichiers)")
        ));
        out.push_str(&format!("Workflow : {}\n", run.title));
        if let Some(request) = &run.request {
            out.push_str(&format!("Demande d'origine : « {request} »\n"));
        }
        out.push_str(&format!("Ton rôle : {}. Compétences : {}.\n", agent.role, agent.skills.join(", ")));
        out.push_str(&skill_section(db, agent).await);

        // Mémoire : conventions et échecs connus d'abord, puis ce que la
        // recherche (mots exacts et proximité de sens) rapproche de la tâche.
        // Les souvenirs rapprochés gardent leur place même quand le socle est
        // plein : sans cette réserve, ils étaient coupés au-delà de 20 lignes.
        let baseline = repo::memory::baseline(db, &project.id, &agent.id).await?;
        let query = format!("{} {}", task.title, task.description);
        let hits = self.recall_memories(&project.id, &query, MEMORY_HITS).await?;
        let room = MEMORY_LINES.saturating_sub(hits.iter().filter(|h| !baseline.iter().any(|b| b.id == h.id)).count());
        let mut memory: Vec<MemoryEntry> = baseline.into_iter().take(room).collect();
        for hit in hits {
            if !memory.iter().any(|m| m.id == hit.id) {
                memory.push(hit);
            }
        }
        if !memory.is_empty() {
            out.push_str("\n## Ce que l'on sait déjà\n");
            for m in memory.iter().take(MEMORY_LINES) {
                out.push_str(&format!("- [{:?}] {}\n", m.kind, m.content));
            }
        }

        if !task.depends_on.is_empty() {
            out.push_str("\n## Résultats des étapes dont tu dépends\n");
            for dep in &task.depends_on {
                if let Ok(t) = repo::tasks::get(db, dep).await {
                    out.push_str(&format!("### {}\n{}\n\n", t.title, tail(t.result.as_deref().unwrap_or("(aucun résultat)"), 1_500)));
                }
            }
        }

        out.push_str("\n## Outils\nTu n'exécutes rien toi-même : tu choisis UNE action à la fois, Atelier l'exécute sous contrôle de permissions et te renvoie le résultat.\n");
        if tools.is_empty() {
            out.push_str("Tu n'as aucun outil : raisonne puis termine avec `finish`.\n");
        }
        for id in tools {
            if let Some(tool) = self.tools.get(id) {
                out.push_str(&format!("- `{id}` — {} Arguments : {}\n", tool.description(), tool.schema()));
            }
        }
        out.push_str(&format!(
            "\n## Règles\n\
             - Chemins relatifs à la racine du projet.\n\
             - `shell.exec` : pas de pipe, de redirection, de `;` ni de `&&` ; entrée standard fermée ; aucune commande interactive.\n\
             - Une action refusée ne sera pas accordée en insistant : adapte-toi ou termine en échec en expliquant pourquoi.\n\
             - Vérifie ce que tu fais (relis, lance les tests) avant de conclure.\n\
             - Quand la tâche est accomplie : `finish`, avec un compte rendu factuel de ce qui a été fait et vérifié.\n\
             - Au plus {MAX_STEPS} actions.\n"
        ));
        Ok(out)
    }
}

/// « Ta méthode » (skill de rôle) puis « Spécificités de cet agent »
/// (surcouche), chacun coupé à son budget. Sections omises si vides ; un
/// slug qui ne correspond plus à aucun skill est ignoré sans faire échouer
/// la tâche — l'agent travaille alors comme avant les skills.
async fn skill_section(db: &atelier_store::Db, agent: &Agent) -> String {
    let skill = match agent.skill_slug.as_deref() {
        Some(slug) => match repo::agent_skills::get(db, slug).await {
            Ok(found) => found,
            Err(e) => {
                tracing::warn!("lecture du skill « {slug} » impossible : {e}");
                None
            }
        },
        None => None,
    };
    let method = skill.map(|s| s.content.trim().to_string()).filter(|c| !c.is_empty());
    let notes = Some(agent.skill_notes.trim()).filter(|n| !n.is_empty());

    let mut out = String::new();
    if method.is_some() || notes.is_some() {
        out.push_str("\n## Ta méthode\n");
    }
    if let Some(method) = method {
        out.push_str(&tail(&method, SKILL_BUDGET));
        out.push('\n');
    }
    if let Some(notes) = notes {
        out.push_str("\n### Spécificités de cet agent\n");
        out.push_str(&tail(notes, SKILL_NOTES_BUDGET));
        out.push('\n');
    }
    out
}

async fn wait_if_paused(paused: &mut watch::Receiver<bool>, cancel: &CancellationToken) -> Result<(), Failure> {
    if *paused.borrow() {
        tokio::select! {
            _ = paused.wait_for(|p| !*p) => {}
            _ = cancel.cancelled() => return Err(Failure::Cancelled),
        }
    }
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    Ok(())
}

fn compact(messages: &mut [Message]) {
    let n = messages.len();
    if n <= KEEP_RECENT + 1 {
        return;
    }
    // Le premier message (la tâche) est toujours conservé intact.
    for m in messages[1..n - KEEP_RECENT].iter_mut() {
        if m.content.chars().count() > 400 {
            let head: String = m.content.chars().take(300).collect();
            m.content = format!("{head}… [tronqué pour économiser le contexte]");
        }
    }
}

fn tail(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let skipped: String = text.chars().skip(count - max).collect();
    format!("… [{} caractères omis]\n{skipped}", count - max)
}

fn served_cost(cost: f64, served_by: &str) -> String {
    if cost > 0.0 {
        format!("{served_by} · équivalent API estimé {cost:.3} $")
    } else {
        served_by.to_string()
    }
}
