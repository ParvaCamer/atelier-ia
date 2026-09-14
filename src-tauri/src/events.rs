//! Transfert moteur → webview.
//!
//! Deux canaux, deux rythmes :
//!   * `world:snapshot` — état agrégé, 8 Hz, seulement quand ça change ;
//!   * `engine:logs`    — lignes de terminal, groupées toutes les 100 ms.
//!
//! Grouper est ce qui permet à un agent bavard de ne pas noyer l'IPC.

use atelier_domain::{DomainEvent, LogLine};
use atelier_engine::Engine;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub const EV_SNAPSHOT: &str = "world:snapshot";
pub const EV_LOGS: &str = "engine:logs";
pub const EV_DOMAIN: &str = "engine:event";

const LOG_BATCH_MS: u64 = 100;
/// Au-delà, on jette : mieux vaut un terminal qui saute des lignes qu'une
/// interface figée. Les lignes restent en base de toute façon.
const LOG_BATCH_MAX: usize = 500;

pub fn spawn_forwarders(app: AppHandle, engine: Arc<Engine>) {
    spawn_snapshots(app.clone(), engine.clone());
    spawn_events(app, engine);
}

fn spawn_snapshots(app: AppHandle, engine: Arc<Engine>) {
    tauri::async_runtime::spawn(async move {
        let mut rx = engine.snapshots();
        while rx.changed().await.is_ok() {
            let snapshot = rx.borrow_and_update().clone();
            let _ = app.emit(EV_SNAPSHOT, snapshot);
        }
    });
}

fn spawn_events(app: AppHandle, engine: Arc<Engine>) {
    tauri::async_runtime::spawn(async move {
        let mut rx = engine.bus().subscribe();
        let mut batch: Vec<LogLine> = Vec::with_capacity(128);
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(LOG_BATCH_MS));

        loop {
            tokio::select! {
                event = rx.recv() => match event {
                    Ok(DomainEvent::Log(line)) => {
                        if batch.len() < LOG_BATCH_MAX {
                            batch.push(line);
                        }
                    }
                    // Les événements métier sont rares : on les transmet
                    // immédiatement, sans attendre le prochain lot.
                    Ok(other) => { let _ = app.emit(EV_DOMAIN, other); }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                },
                _ = ticker.tick() => {
                    if !batch.is_empty() {
                        let _ = app.emit(EV_LOGS, std::mem::take(&mut batch));
                    }
                }
            }
        }
    });
}
