//! Historique, mémoire, planifications.

use crate::state::{err, AppState, CmdResult};
use atelier_domain::*;
use chrono::{DateTime, Utc};
use tauri::State;

#[tauri::command]
pub async fn list_runs(state: State<'_, AppState>, filter: RunFilter) -> CmdResult<Vec<RunSummary>> {
    state.engine.list_runs(filter).await.map_err(err)
}

#[tauri::command]
pub async fn run_detail(state: State<'_, AppState>, run_id: RunId) -> CmdResult<RunDetail> {
    state.engine.run_detail(&run_id).await.map_err(err)
}

#[tauri::command]
pub async fn list_memories(state: State<'_, AppState>, filter: MemoryFilter) -> CmdResult<Vec<MemoryView>> {
    state.engine.list_memories(filter).await.map_err(err)
}

#[tauri::command]
pub async fn save_memory(state: State<'_, AppState>, entry: MemoryEntry) -> CmdResult<MemoryEntry> {
    state.engine.save_memory(entry).await.map_err(err)
}

#[tauri::command]
pub async fn delete_memory(state: State<'_, AppState>, memory_id: MemoryId) -> CmdResult<()> {
    state.engine.delete_memory(&memory_id).await.map_err(err)
}

#[tauri::command]
pub async fn list_schedules(state: State<'_, AppState>) -> CmdResult<Vec<Schedule>> {
    state.engine.list_schedules().await.map_err(err)
}

#[tauri::command]
pub async fn save_schedule(state: State<'_, AppState>, schedule: Schedule) -> CmdResult<Schedule> {
    state.engine.save_schedule(schedule).await.map_err(err)
}

#[tauri::command]
pub async fn delete_schedule(state: State<'_, AppState>, schedule_id: ScheduleId) -> CmdResult<()> {
    state.engine.delete_schedule(&schedule_id).await.map_err(err)
}

#[tauri::command]
pub fn preview_schedule(state: State<'_, AppState>, cron: String) -> CmdResult<Vec<DateTime<Utc>>> {
    state.engine.preview_schedule(&cron, 3).map_err(err)
}

#[tauri::command]
pub async fn run_schedule_now(state: State<'_, AppState>, schedule_id: ScheduleId) -> CmdResult<RunId> {
    state.engine.run_schedule_now(&schedule_id).await.map_err(err)
}

#[tauri::command]
pub async fn list_watches(state: State<'_, AppState>) -> CmdResult<Vec<FileWatch>> {
    state.engine.list_watches().await.map_err(err)
}

#[tauri::command]
pub async fn save_watch(state: State<'_, AppState>, watch: FileWatch) -> CmdResult<FileWatch> {
    state.engine.save_watch(watch).await.map_err(err)
}

#[tauri::command]
pub async fn delete_watch(state: State<'_, AppState>, watch_id: WatchId) -> CmdResult<()> {
    state.engine.delete_watch(&watch_id).await.map_err(err)
}

#[tauri::command]
pub async fn cost_summary(state: State<'_, AppState>) -> CmdResult<CostSummary> {
    state.engine.cost_summary().await.map_err(err)
}
