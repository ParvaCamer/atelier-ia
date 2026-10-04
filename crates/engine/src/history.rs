//! Historique consultable : ce qui a été demandé, qui a fait quoi, avec quelles
//! décisions de permission, et avec quel résultat.

use crate::Engine;
use atelier_domain::*;
use atelier_store::repo;
use std::collections::HashMap;

impl Engine {
    pub async fn list_runs(&self, filter: RunFilter) -> anyhow::Result<Vec<RunSummary>> {
        Ok(repo::runs::list_summaries(self.db(), &filter).await?)
    }

    pub async fn run_detail(&self, run_id: &RunId) -> anyhow::Result<RunDetail> {
        let db = self.db();
        let summary = repo::runs::get_summary(db, run_id).await?;
        let names: HashMap<AgentId, String> = repo::agents::list(db).await?.into_iter().map(|a| (a.id, a.name)).collect();

        let mut tasks = Vec::new();
        for task in repo::tasks::list_by_run(db, run_id).await? {
            tasks.push(TaskDetail {
                agent_name: names.get(&task.agent_id).cloned().unwrap_or_else(|| "agent supprimé".into()),
                tool_calls: repo::tool_calls::list_by_task(db, &task.id).await?,
                task,
            });
        }
        let handoffs = repo::handoffs::list_by_run(db, run_id).await?;
        Ok(RunDetail { summary, tasks, handoffs })
    }
}
