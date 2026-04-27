use super::AppStateArg;
use crate::error::Result;
use serde::Serialize;

#[derive(Serialize)]
pub struct RestStatus {
    pub running: bool,
    pub bind: Option<String>,
    pub port: Option<i64>,
}

#[tauri::command]
pub async fn rest_api_start(state: AppStateArg<'_>) -> Result<RestStatus> {
    let mut handle_guard = state.rest_handle.lock().await;
    if handle_guard.is_some() {
        return rest_api_status_inner(&state).await;
    }
    let handle = crate::rest_api::start(state.inner().clone()).await?;
    let bind = handle.bind.clone();
    let port = handle.port;
    *handle_guard = Some(handle);
    Ok(RestStatus { running: true, bind: Some(bind), port: Some(port) })
}

#[tauri::command]
pub async fn rest_api_stop(state: AppStateArg<'_>) -> Result<RestStatus> {
    let mut handle_guard = state.rest_handle.lock().await;
    if let Some(h) = handle_guard.take() {
        h.shutdown.send(()).ok();
    }
    Ok(RestStatus { running: false, bind: None, port: None })
}

#[tauri::command]
pub async fn rest_api_status(state: AppStateArg<'_>) -> Result<RestStatus> {
    rest_api_status_inner(&state).await
}

async fn rest_api_status_inner(state: &AppStateArg<'_>) -> Result<RestStatus> {
    let g = state.rest_handle.lock().await;
    Ok(match g.as_ref() {
        Some(h) => RestStatus { running: true, bind: Some(h.bind.clone()), port: Some(h.port) },
        None => RestStatus { running: false, bind: None, port: None },
    })
}
