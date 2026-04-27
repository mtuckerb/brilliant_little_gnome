use super::AppStateArg;
use crate::error::Result;
use crate::models::UserPreferences;
use serde::Deserialize;

#[tauri::command]
pub async fn get_prefs(state: AppStateArg<'_>) -> Result<UserPreferences> {
    let prefs = sqlx::query_as::<_, UserPreferences>(
        "SELECT id, display_name, time_zone, brightspace_host, api_enabled, api_key, api_listen_all, api_port, jwt_secret, historic_gpa, historic_units, default_semester, brightspace_uid, brightspace_user_id, last_login_at FROM user_preferences LIMIT 1",
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(prefs)
}

#[derive(Deserialize, Default)]
pub struct PrefsPatch {
    pub display_name: Option<String>,
    pub time_zone: Option<String>,
    pub historic_gpa: Option<f64>,
    pub historic_units: Option<f64>,
    pub default_semester: Option<String>,
    pub api_enabled: Option<bool>,
    pub api_listen_all: Option<bool>,
    pub api_port: Option<i64>,
}

#[tauri::command]
pub async fn update_prefs(state: AppStateArg<'_>, patch: PrefsPatch) -> Result<UserPreferences> {
    // Apply each provided field. Keep simple: separate UPDATEs for clarity.
    let pool = &state.pool;
    if let Some(v) = patch.display_name {
        sqlx::query("UPDATE user_preferences SET display_name = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v).execute(pool).await?;
    }
    if let Some(v) = patch.time_zone {
        sqlx::query("UPDATE user_preferences SET time_zone = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v).execute(pool).await?;
    }
    if let Some(v) = patch.historic_gpa {
        sqlx::query("UPDATE user_preferences SET historic_gpa = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v).execute(pool).await?;
    }
    if let Some(v) = patch.historic_units {
        sqlx::query("UPDATE user_preferences SET historic_units = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v).execute(pool).await?;
    }
    if let Some(v) = patch.default_semester {
        sqlx::query("UPDATE user_preferences SET default_semester = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v).execute(pool).await?;
    }
    if let Some(v) = patch.api_enabled {
        sqlx::query("UPDATE user_preferences SET api_enabled = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v as i64).execute(pool).await?;
    }
    if let Some(v) = patch.api_listen_all {
        sqlx::query("UPDATE user_preferences SET api_listen_all = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v as i64).execute(pool).await?;
    }
    if let Some(v) = patch.api_port {
        sqlx::query("UPDATE user_preferences SET api_port = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v).execute(pool).await?;
    }
    get_prefs(state).await
}
