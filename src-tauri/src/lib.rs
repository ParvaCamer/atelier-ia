//! Coquille desktop.
//!
//! Cette couche est délibérément mince : elle ouvre la base, démarre le
//! moteur, expose des commandes et retransmet des événements. Rien d'autre.
//! C'est ce qui rend la coquille remplaçable sans toucher au moteur.

mod commands;
mod events;
mod state;

use atelier_engine::Engine;
use atelier_store::{seed, Db};
use atelier_tools::pty::PtyManager;
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "atelier=debug,warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let db_path = data_dir.join("atelier.db");
            tracing::info!("base locale: {}", db_path.display());

            // Démarrage synchrone volontaire : sans base ni moteur,
            // afficher une fenêtre n'aurait aucun sens.
            let engine = tauri::async_runtime::block_on(async move {
                let db = Db::open(&db_path).await?;
                if seed::run_if_empty(&db).await? {
                    tracing::info!("monde initial créé");
                }
                seed::ensure_builtin_workflows(&db).await?;
                seed::ensure_builtin_providers(&db).await?;
                Engine::start(db).await
            })?;

            events::spawn_forwarders(app.handle().clone(), engine.clone());

            // En tâche de fond : ne retarde jamais l'ouverture de la fenêtre.
            let starter = engine.clone();
            tauri::async_runtime::spawn(async move { starter.start_ollama_if_configured().await });
            app.manage(AppState { engine, pty: PtyManager::default() });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::query::list_projects,
            commands::query::list_agents,
            commands::query::list_workflows,
            commands::query::get_snapshot,
            commands::query::tail_logs,
            commands::query::list_recent_runs,
            commands::query::list_run_tasks,
            commands::query::pending_approvals,
            commands::query::list_grants,
            commands::control::run_command,
            commands::control::launch_workflow,
            commands::control::resolve_approval,
            commands::control::control_task,
            commands::control::submit_request,
            commands::terminal::pty_open,
            commands::terminal::pty_write,
            commands::terminal::pty_resize,
            commands::terminal::pty_close,
            commands::config::list_all_projects,
            commands::config::save_project,
            commands::config::save_agent,
            commands::config::delete_agent,
            commands::config::agent_grants,
            commands::config::save_agent_grants,
            commands::config::grant_preset,
            commands::config::save_workflow,
            commands::config::check_workflow,
            commands::config::delete_workflow,
            commands::config::list_provider_configs,
            commands::config::list_model_routes,
            commands::config::save_provider,
            commands::config::save_route,
            commands::config::delete_route,
            commands::config::test_route,
            commands::config::provider_health,
            commands::config::start_ollama,
            commands::config::get_settings,
            commands::config::save_settings,
            commands::config::tool_catalog,
            commands::phase4::list_runs,
            commands::phase4::run_detail,
            commands::phase4::list_memories,
            commands::phase4::save_memory,
            commands::phase4::delete_memory,
            commands::phase4::list_schedules,
            commands::phase4::save_schedule,
            commands::phase4::delete_schedule,
            commands::phase4::preview_schedule,
            commands::phase4::run_schedule_now,
        ])
        .run(tauri::generate_context!())
        .expect("démarrage de l'application impossible");
}
