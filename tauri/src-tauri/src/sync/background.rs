// Periodic background sync loop. Mirrors the Ruby app's sync poller.

use crate::state::AppState;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

pub async fn run_periodic_loop(state: Arc<AppState>, _app: AppHandle) {
    // Initial sync on launch when authenticated.
    if state.client.is_configured() {
        if let Err(e) = super::sync_all(state.clone(), false).await {
            tracing::warn!("initial sync failed: {}", e);
        }
    }

    let mut ticker = tokio::time::interval(Duration::from_secs(15 * 60));
    ticker.tick().await; // skip immediate tick

    loop {
        ticker.tick().await;
        if !state.client.is_configured() {
            continue;
        }
        if let Err(e) = super::sync_all(state.clone(), false).await {
            tracing::warn!("periodic sync failed: {}", e);
            state.events.sync_error(&e.to_string());
        }
    }
}
