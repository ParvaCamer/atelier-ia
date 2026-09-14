//! Terminal interactif. La sortie passe par un `Channel` Tauri dédié à la
//! session plutôt que par le bus d'événements global : un `cat` sur un gros
//! fichier ne doit pas concurrencer les snapshots du monde.

use crate::state::{err, AppState, CmdResult};
use atelier_domain::{ProjectId, PtyId};
use atelier_store::repo;
use serde::Serialize;
use tauri::{ipc::Channel, State};

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PtyEvent {
    Output { data: String },
    Exit,
}

#[tauri::command]
pub async fn pty_open(
    state: State<'_, AppState>,
    cols: u16,
    rows: u16,
    project_id: Option<ProjectId>,
    events: Channel<PtyEvent>,
) -> CmdResult<PtyId> {
    // Le shell s'ouvre dans le dossier du projet de l'agent sélectionné.
    let cwd = match project_id {
        Some(id) => repo::projects::get(state.engine.db(), &id).await.ok().and_then(|p| p.root_path),
        None => None,
    };
    let output = events.clone();
    state
        .pty
        .open(
            cols.max(10),
            rows.max(4),
            cwd.as_deref().map(std::path::Path::new),
            Box::new(move |data| {
                let _ = output.send(PtyEvent::Output { data });
            }),
            Box::new(move || {
                let _ = events.send(PtyEvent::Exit);
            }),
        )
        .map_err(err)
}

#[tauri::command]
pub fn pty_write(state: State<'_, AppState>, id: PtyId, data: String) -> CmdResult<()> {
    state.pty.write(&id, &data).map_err(err)
}

#[tauri::command]
pub fn pty_resize(state: State<'_, AppState>, id: PtyId, cols: u16, rows: u16) -> CmdResult<()> {
    state.pty.resize(&id, cols.max(10), rows.max(4)).map_err(err)
}

#[tauri::command]
pub fn pty_close(state: State<'_, AppState>, id: PtyId) -> CmdResult<()> {
    state.pty.close(&id);
    Ok(())
}
