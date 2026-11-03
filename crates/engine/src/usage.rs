//! Consommation des modèles : consignée à chaque appel, telle que le
//! fournisseur l'annonce. Aucun appel supplémentaire pour mesurer.
//!
//! Plafonds : vérifiés **avant** chaque appel, contre le cumul déjà
//! consigné. Un appel en cours n'est jamais coupé, le dernier peut donc
//! dépasser un peu ; c'est le prix d'une vérification qui ne devine rien.
//! Seuls les coûts annoncés comptent : un appel sans coût (Ollama, ou un
//! fournisseur qui ne le dit pas) ne rapproche d'aucun plafond.

use crate::Engine;
use atelier_domain::*;
use atelier_providers::Completion;
use atelier_store::repo::{self, usage::UsageRecord};
use chrono::{Datelike, Local, TimeZone, Utc};

impl Engine {
    /// Renvoie l'identifiant de la ligne, pour rattacher plus tard à un run
    /// les appels faits avant sa création (aiguillage, planification).
    pub(crate) async fn record_usage(&self, run: Option<&RunId>, task: Option<&TaskId>, purpose: &str, c: &Completion) -> Option<String> {
        let record = UsageRecord {
            run_id: run,
            task_id: task,
            purpose,
            served_by: &c.served_by,
            input_tokens: c.usage.input_tokens.min(i64::MAX as u64) as i64,
            output_tokens: c.usage.output_tokens.min(i64::MAX as u64) as i64,
            cost_usd: c.usage.cost_usd.filter(|v| v.is_finite() && *v >= 0.0),
        };
        match repo::usage::insert_returning(self.db(), &record).await {
            Ok(id) => Some(id),
            Err(e) => {
                // Ne jamais faire échouer une tâche pour une ligne de comptabilité.
                tracing::warn!("consommation non consignée : {e}");
                None
            }
        }
    }

    /// Cumul du mois en cours, en heure locale.
    pub async fn cost_summary(&self) -> anyhow::Result<CostSummary> {
        let start = month_start();
        let (total, by_provider) = repo::usage::since(self.db(), start).await?;
        let settings = self.settings().await?;
        Ok(CostSummary {
            since: start,
            total,
            by_provider,
            month_cap_usd: settings.month_cost_cap_usd,
            run_cap_usd: settings.run_cost_cap_usd,
        })
    }

    /// Un appel de modèle peut-il partir ? `run` absent : appel hors run
    /// (aiguillage, planification, brouillon…), seul le plafond mensuel
    /// s'applique. Fail-closed : un cumul illisible vaut refus, sans quoi une
    /// base en défaut lèverait tous les plafonds d'un coup.
    pub(crate) async fn budget_check(&self, run: Option<&RunId>) -> Result<(), String> {
        let unreadable = |e: &dyn std::fmt::Display| format!("consommation illisible, appel de modèle refusé par précaution : {e}");
        let settings = self.settings().await.map_err(|e| unreadable(&e))?;
        if let Some(cap) = settings.month_cost_cap_usd {
            let (total, _) = repo::usage::since(self.db(), month_start()).await.map_err(|e| unreadable(&e))?;
            let spent = total.cost_usd.unwrap_or(0.0);
            if spent >= cap {
                return Err(format!(
                    "plafond mensuel atteint : {} dépensés ce mois pour un plafond de {}. \
                     Relève-le dans Réglages › IA, ou attends le 1er du mois",
                    usd(spent),
                    usd(cap)
                ));
            }
        }
        if let (Some(cap), Some(run)) = (settings.run_cost_cap_usd, run) {
            let spent = repo::usage::for_run(self.db(), run).await.map_err(|e| unreadable(&e))?.cost_usd.unwrap_or(0.0);
            if spent >= cap {
                return Err(format!(
                    "plafond du run atteint : {} dépensés pour un plafond de {}. \
                     Relève-le dans Réglages › IA : la tâche reprendra d'elle-même",
                    usd(spent),
                    usd(cap)
                ));
            }
        }
        Ok(())
    }

    /// Variante pour les appels qui renvoient une erreur ordinaire.
    pub(crate) async fn ensure_budget(&self, run: Option<&RunId>) -> anyhow::Result<()> {
        self.budget_check(run).await.map_err(|e| anyhow::anyhow!(e))
    }

    /// Met une tâche en pause faute de budget. Elle reprendra seule si un
    /// plafond relevé le permet (`resume_budget_holds`), ou à la main.
    pub(crate) async fn hold_for_budget(&self, task: &Task, reason: &str) -> anyhow::Result<()> {
        self.system_log(&task.agent_id, &task.project_id, Some(&task.id), &format!("⏸ {reason}"));
        self.budget_holds.lock().expect("verrou des pauses budgétaires").insert(task.id.clone());
        self.control_task(&task.id, TaskControl::Pause).await
    }

    /// Après un changement de plafond : relance les tâches mises en pause
    /// faute de budget, si leur run peut de nouveau payer un appel.
    pub(crate) async fn resume_budget_holds(&self) {
        let held: Vec<TaskId> = self.budget_holds.lock().expect("verrou des pauses budgétaires").iter().cloned().collect();
        for id in held {
            let task = match repo::tasks::get(self.db(), &id).await {
                Ok(t) if t.status == TaskStatus::Paused => t,
                // Reprise, arrêtée ou disparue entre-temps : plus rien à faire.
                _ => {
                    self.budget_holds.lock().expect("verrou des pauses budgétaires").remove(&id);
                    continue;
                }
            };
            if self.budget_check(Some(&task.run_id)).await.is_err() {
                continue;
            }
            self.budget_holds.lock().expect("verrou des pauses budgétaires").remove(&id);
            if let Err(e) = self.control_task(&id, TaskControl::Resume).await {
                tracing::warn!("reprise après relèvement du plafond impossible : {e}");
            }
        }
    }
}

/// Bornes d'un plafond saisi : au-delà, c'est une faute de frappe.
pub const MAX_COST_CAP_USD: f64 = 100_000.0;

/// `None` passe tel quel ; sinon un montant positif et fini, arrondi au cent.
pub(crate) fn validate_cap(label: &str, cap: Option<f64>) -> anyhow::Result<Option<f64>> {
    let Some(v) = cap else { return Ok(None) };
    if !v.is_finite() || v <= 0.0 {
        anyhow::bail!("plafond {label} invalide : indique un montant en dollars supérieur à 0, ou laisse le champ vide pour ne rien plafonner");
    }
    if v > MAX_COST_CAP_USD {
        anyhow::bail!("plafond {label} de {} : au-delà de {}, c'est sans doute une faute de frappe", usd(v), usd(MAX_COST_CAP_USD));
    }
    Ok(Some((v * 100.0).round() / 100.0))
}

fn month_start() -> chrono::DateTime<Utc> {
    let now = Local::now();
    Local
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

/// « 12,40 $ » : les messages sont lus en français.
pub(crate) fn usd(v: f64) -> String {
    format!("{v:.2} $").replace('.', ",")
}
