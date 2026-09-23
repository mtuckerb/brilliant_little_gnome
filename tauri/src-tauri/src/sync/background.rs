// Periodic background sync loop. Mirrors the Ruby app's sync poller.

use crate::state::AppState;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

pub async fn run_periodic_loop(state: Arc<AppState>, _app: AppHandle) {
    #[cfg(feature = "p2p")]
    recover_missing_session(&state).await;
    // Initial sync on launch when authenticated.
    if state.client.is_configured() {
        if let Err(e) = super::sync_all(state.clone(), false).await {
            tracing::warn!("initial sync failed: {}", e);
        }
    }

    let mut ticker = tokio::time::interval(Duration::from_secs(15 * 60));
    ticker.tick().await; // skip immediate tick
    let mut auth_retry = tokio::time::interval(Duration::from_secs(30));
    auth_retry.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {},
            _ = auth_retry.tick() => {
                #[cfg(feature = "p2p")]
                if recover_missing_session(&state).await {
                    if let Err(e) = super::sync_all(state.clone(), false).await {
                        tracing::warn!("sync after peer authentication failed: {e}");
                    }
                }
                continue;
            }
        }
        if !state.client.is_configured() {
            continue;
        }
        if let Err(e) = super::sync_all(state.clone(), false).await {
            tracing::warn!("periodic sync failed: {}", e);
            state.events.sync_error(&e.to_string());
        }
    }
}

#[cfg(feature = "p2p")]
async fn recover_missing_session(state: &AppState) -> bool {
    if state.sync_engine().is_none()
        || (state.client.is_configured() && !state.client.is_degraded())
    {
        return false;
    }
    let rejected = state.client.cookie_clone().unwrap_or_default();
    state.recover_credentials_from_peers(&rejected).await
}
