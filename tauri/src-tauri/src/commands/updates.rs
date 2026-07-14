//! Desktop OTA self-update commands (wraps tauri-plugin-updater). On mobile
//! these are stubs — the App Store / TestFlight handles updates there.

use crate::error::{AppError, Result};
use serde::Serialize;
use tauri::AppHandle;

#[derive(Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

#[cfg(desktop)]
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<Option<UpdateInfo>> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app
        .updater()
        .map_err(|e| AppError::Other(format!("updater unavailable: {e}")))?;
    match updater.check().await {
        Ok(Some(u)) => Ok(Some(UpdateInfo {
            version: u.version.clone(),
            current_version: u.current_version.clone(),
            notes: u.body.clone(),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(AppError::Other(format!("update check failed: {e}"))),
    }
}

/// Download + install the pending update, then restart the app. Diverges on
/// success (the process is replaced), so callers won't see an Ok.
#[cfg(desktop)]
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<()> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app
        .updater()
        .map_err(|e| AppError::Other(format!("updater unavailable: {e}")))?;
    let update = updater
        .check()
        .await
        .map_err(|e| AppError::Other(format!("update check failed: {e}")))?
        .ok_or_else(|| AppError::Other("No update available".into()))?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| AppError::Other(format!("update install failed: {e}")))?;
    app.restart();
}

#[cfg(not(desktop))]
#[tauri::command]
pub async fn check_for_updates(_app: AppHandle) -> Result<Option<UpdateInfo>> {
    // Mobile ships through the App Store / TestFlight.
    Ok(None)
}

#[cfg(not(desktop))]
#[tauri::command]
pub async fn install_update(_app: AppHandle) -> Result<()> {
    Err(AppError::Other(
        "On mobile, updates are delivered through the App Store.".into(),
    ))
}
