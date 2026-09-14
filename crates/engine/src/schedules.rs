//! Planifications : lancer un workflow ou une demande selon un calendrier cron.
//!
//! Limite assumée : **ne tourne que quand Atelier est ouvert**. Une échéance
//! manquée (application fermée, Mac en veille) est rattrapée une fois au
//! prochain passage si la planification l'autorise, sinon ignorée — jamais
//! rejouée autant de fois qu'elle a été manquée.

use crate::Engine;
use atelier_domain::*;
use atelier_store::repo;
use chrono::{DateTime, Local, Utc};
use croner::Cron;
use std::sync::Arc;
use std::time::Duration;

const TICK: Duration = Duration::from_secs(30);
/// Au-delà de ce retard, l'échéance a été manquée (et non simplement atteinte
/// entre deux passages).
const LATE_AFTER: chrono::Duration = chrono::Duration::minutes(2);

pub fn parse_cron(expr: &str) -> anyhow::Result<Cron> {
    let expr = expr.trim();
    if expr.split_whitespace().count() != 5 {
        anyhow::bail!("5 champs attendus : minute heure jour-du-mois mois jour-de-semaine (ex. « 0 9 * * 1 » = lundi 9 h)");
    }
    expr.parse::<Cron>().map_err(|e| anyhow::anyhow!("expression cron invalide : {e}"))
}

/// Prochaine échéance strictement après `after`, calculée en heure locale :
/// « 9 h » veut dire 9 h à ta montre, changements d'heure compris.
pub fn next_after(cron: &Cron, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    cron.find_next_occurrence(&after.with_timezone(&Local), false).ok().map(|d| d.with_timezone(&Utc))
}

pub fn preview(expr: &str, from: DateTime<Utc>, count: usize) -> anyhow::Result<Vec<DateTime<Utc>>> {
    let cron = parse_cron(expr)?;
    let mut out = Vec::with_capacity(count);
    let mut cursor = from;
    for _ in 0..count {
        match next_after(&cron, cursor) {
            Some(next) => {
                out.push(next);
                cursor = next;
            }
            None => break,
        }
    }
    if out.is_empty() {
        anyhow::bail!("cette expression ne se déclenche jamais");
    }
    Ok(out)
}

impl Engine {
    pub async fn list_schedules(&self) -> anyhow::Result<Vec<Schedule>> {
        Ok(repo::schedules::list(self.db()).await?)
    }

    pub fn preview_schedule(&self, cron: &str, count: usize) -> anyhow::Result<Vec<DateTime<Utc>>> {
        preview(cron, Utc::now(), count.clamp(1, 10))
    }

    pub async fn save_schedule(&self, draft: Schedule) -> anyhow::Result<Schedule> {
        let db = self.db();
        let name = draft.name.trim().to_string();
        if name.is_empty() {
            anyhow::bail!("le nom est obligatoire");
        }
        let cron_expr = draft.cron.split_whitespace().collect::<Vec<_>>().join(" ");
        let cron = parse_cron(&cron_expr)?;

        let target = match draft.target {
            ScheduleTarget::Workflow { workflow_id } => {
                let wf = repo::workflows::get(db, &workflow_id).await.map_err(|_| anyhow::anyhow!("workflow introuvable"))?;
                if !wf.enabled {
                    anyhow::bail!("le workflow « {} » est désactivé", wf.name);
                }
                ScheduleTarget::Workflow { workflow_id }
            }
            ScheduleTarget::Request { text, project_id } => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    anyhow::bail!("la demande est vide");
                }
                if let Some(id) = &project_id {
                    let p = repo::projects::get(db, id).await.map_err(|_| anyhow::anyhow!("projet introuvable"))?;
                    if p.archived {
                        anyhow::bail!("le projet {} est archivé", p.name);
                    }
                }
                ScheduleTarget::Request { text, project_id }
            }
        };

        let existing = if draft.id.as_str().is_empty() { None } else { repo::schedules::get(db, &draft.id).await.ok() };
        let schedule = Schedule {
            id: existing.as_ref().map(|s| s.id.clone()).unwrap_or_else(ScheduleId::new),
            name,
            target,
            cron: cron_expr,
            enabled: draft.enabled,
            run_missed: draft.run_missed,
            last_run_at: existing.as_ref().and_then(|s| s.last_run_at),
            last_run_id: existing.as_ref().and_then(|s| s.last_run_id.clone()),
            last_outcome: existing.as_ref().and_then(|s| s.last_outcome),
            last_error: existing.as_ref().and_then(|s| s.last_error.clone()),
            // Recalculée à chaque enregistrement : modifier l'horaire ne doit
            // pas déclencher une exécution « en retard » sur l'ancien.
            next_run_at: draft.enabled.then(|| next_after(&cron, Utc::now())).flatten(),
            created_at: existing.as_ref().map(|s| s.created_at).unwrap_or_else(Utc::now),
        };
        repo::schedules::upsert(db, &schedule).await?;
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(schedule)
    }

    pub async fn delete_schedule(&self, id: &ScheduleId) -> anyhow::Result<()> {
        repo::schedules::delete(self.db(), id).await?;
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(())
    }

    /// « Exécuter maintenant » : ne décale pas la prochaine échéance.
    pub async fn run_schedule_now(&self, id: &ScheduleId) -> anyhow::Result<RunId> {
        let schedule = repo::schedules::get(self.db(), id).await?;
        let _guard = self.schedule_lock.lock().await;
        match self.fire(&schedule, Utc::now(), schedule.next_run_at).await? {
            (ScheduleOutcome::Launched, Some(run)) => Ok(run),
            (_, _) => {
                let s = repo::schedules::get(self.db(), id).await?;
                anyhow::bail!("{}", s.last_error.unwrap_or_else(|| "non lancée".into()))
            }
        }
    }

    pub(crate) fn spawn_schedule_ticker(self: Arc<Self>) {
        tokio::spawn(async move {
            // Court délai au démarrage : le rattrapage des échéances manquées
            // ne doit pas concurrencer l'ouverture de la fenêtre.
            tokio::time::sleep(Duration::from_secs(5)).await;
            loop {
                if let Err(e) = self.fire_due(Utc::now()).await {
                    tracing::error!("planifications : {e}");
                }
                tokio::time::sleep(TICK).await;
            }
        });
    }

    /// Traite les échéances atteintes à `now`. Horloge injectée : c'est ce qui
    /// rend les planifications testables sans attendre un vrai lundi 9 h.
    pub async fn fire_due(&self, now: DateTime<Utc>) -> anyhow::Result<Vec<(ScheduleId, ScheduleOutcome)>> {
        // Deux passages simultanés pourraient lancer deux fois la même échéance.
        let Ok(_guard) = self.schedule_lock.try_lock() else { return Ok(Vec::new()) };
        let mut outcomes = Vec::new();

        for schedule in repo::schedules::due(self.db(), now).await? {
            let next = parse_cron(&schedule.cron).ok().and_then(|c| next_after(&c, now));
            // Avancer l'échéance AVANT de lancer : un plantage pendant la
            // planification ne doit pas provoquer une relance en boucle.
            repo::schedules::set_next(self.db(), &schedule.id, next).await?;

            let late = schedule.next_run_at.is_some_and(|t| now - t > LATE_AFTER);
            if late && !schedule.run_missed {
                repo::schedules::record(
                    self.db(), &schedule.id, now, None, ScheduleOutcome::Skipped,
                    Some("échéance manquée pendant qu'Atelier était fermé (rattrapage désactivé)"), next,
                ).await?;
                outcomes.push((schedule.id.clone(), ScheduleOutcome::Skipped));
                continue;
            }

            let (outcome, _) = self.fire(&schedule, now, next).await?;
            outcomes.push((schedule.id.clone(), outcome));
        }
        Ok(outcomes)
    }

    async fn fire(&self, schedule: &Schedule, now: DateTime<Utc>, next: Option<DateTime<Utc>>) -> anyhow::Result<(ScheduleOutcome, Option<RunId>)> {
        let db = self.db();

        // Pas de chevauchement : une sauvegarde encore en cours ne doit pas
        // être relancée par-dessus elle-même.
        if let Some(previous) = &schedule.last_run_id {
            if let Ok(run) = repo::runs::get(db, previous).await {
                if matches!(run.status, RunStatus::Planning | RunStatus::Running | RunStatus::Paused) {
                    repo::schedules::record(db, &schedule.id, now, None, ScheduleOutcome::Skipped, Some("exécution précédente encore en cours"), next).await?;
                    return Ok((ScheduleOutcome::Skipped, None));
                }
            }
        }

        self.orchestrator_log(None, format!("⏰ planification « {} »", schedule.name));
        let label = format!("planification « {} »", schedule.name);
        let launched = match &schedule.target {
            ScheduleTarget::Workflow { workflow_id } => self.launch_workflow_with(workflow_id, Some(&label)).await,
            ScheduleTarget::Request { text, project_id } => self.submit_request(text, project_id.as_ref()).await,
        };

        match launched {
            Ok(run) => {
                repo::runs::set_schedule(db, &run, &schedule.id).await?;
                repo::schedules::record(db, &schedule.id, now, Some(&run), ScheduleOutcome::Launched, None, next).await?;
                Ok((ScheduleOutcome::Launched, Some(run)))
            }
            Err(e) => {
                let message = e.to_string();
                self.orchestrator_log(None, format!("⏰ « {} » n'a pas pu démarrer : {message}", schedule.name));
                repo::schedules::record(db, &schedule.id, now, None, ScheduleOutcome::Error, Some(&message), next).await?;
                Ok((ScheduleOutcome::Error, None))
            }
        }
    }
}
