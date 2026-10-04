//! Consommation des modèles : consignée à chaque appel, telle que le
//! fournisseur l'annonce. Aucun appel supplémentaire pour mesurer.

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
        let now = Local::now();
        let start = Local
            .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
            .earliest()
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        let (total, by_provider) = repo::usage::since(self.db(), start).await?;
        Ok(CostSummary { since: start, total, by_provider })
    }
}
