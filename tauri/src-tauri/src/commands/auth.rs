use super::AppStateArg;
use crate::error::Result;
use crate::models::AuthStatus;
use serde::Deserialize;

#[tauri::command]
pub async fn auth_status(state: AppStateArg<'_>) -> Result<AuthStatus> {
    let c = &state.client;
    Ok(AuthStatus {
        authenticated: c.is_configured(),
        degraded: false,
        host: c.host_clone(),
        user_id: c.user_id_clone(),
        uid: c.uid_clone(),
    })
}

#[derive(Deserialize)]
pub struct SetupCookiesArgs {
    pub host: String,
    pub cookie: String,
    pub uid: Option<String>,
    pub user_id: Option<String>,
}

#[tauri::command]
pub async fn setup_cookies(state: AppStateArg<'_>, args: SetupCookiesArgs) -> Result<()> {
    state
        .client
        .store_credentials(&state.pool, &args.host, &args.cookie, args.uid.as_deref(), args.user_id.as_deref())
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn clear_auth(state: AppStateArg<'_>) -> Result<()> {
    state.client.clear_credentials(&state.pool).await
}
