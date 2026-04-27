use super::AppStateArg;
use crate::error::Result;
use crate::models::SyncStatus;

#[tauri::command]
pub async fn sync_status(state: AppStateArg<'_>) -> Result<SyncStatus> {
    Ok(state.sync_status.read().clone())
}

#[tauri::command]
pub async fn sync_all(state: AppStateArg<'_>) -> Result<()> {
    crate::sync::sync_all(state.inner().clone()).await
}

#[tauri::command]
pub async fn sync_course(state: AppStateArg<'_>, course_id: String) -> Result<()> {
    crate::sync::sync_course(state.inner().clone(), &course_id).await
}
