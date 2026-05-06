use super::AppStateArg;
use crate::error::Result;
use crate::models::UserPreferences;
use serde::Deserialize;

#[tauri::command]
pub async fn get_prefs(state: AppStateArg<'_>) -> Result<UserPreferences> {
    let prefs = sqlx::query_as::<_, UserPreferences>(
        "SELECT id, display_name, time_zone, brightspace_host, api_enabled, api_key, api_listen_all, api_port, jwt_secret, historic_gpa, historic_units, default_semester, brightspace_uid, brightspace_user_id, last_login_at, calendar_show_empty_days FROM user_preferences LIMIT 1",
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
    pub calendar_show_empty_days: Option<bool>,
}

#[tauri::command]
pub async fn update_prefs(state: AppStateArg<'_>, patch: PrefsPatch) -> Result<UserPreferences> {
    // Apply each provided field. Keep simple: separate UPDATEs for clarity.
    let pool = &state.pool;
    if let Some(v) = patch.display_name.as_ref() {
        sqlx::query("UPDATE user_preferences SET display_name = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v).execute(pool).await?;
    }
    if let Some(v) = patch.time_zone.as_ref() {
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
    if let Some(v) = patch.default_semester.as_ref() {
        sqlx::query("UPDATE user_preferences SET default_semester = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v).execute(pool).await?;
    }
    // The api_* triplet is intentionally NOT mirrored into Loro:
    // they're per-device (each device runs its own optional REST
    // server on its own port + key). Same applies if they synced,
    // toggling on one device would silently start a server on every
    // device, which is the opposite of what the user wants.
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
    if let Some(v) = patch.calendar_show_empty_days {
        sqlx::query("UPDATE user_preferences SET calendar_show_empty_days = ?, updated_at = CURRENT_TIMESTAMP")
            .bind(v as i64).execute(pool).await?;
    }

    // T-016: mirror Class-B prefs into Loro. `api_*` stays local-only
    // (see comment above).
    #[cfg(feature = "p2p")]
    if let Some(engine) = state.sync_engine() {
        use crate::p2p::bridge::{LocalChange, PrefField};
        let bridge = engine.bridge();
        let push = |field: PrefField| {
            let bridge = bridge.clone();
            async move {
                if let Err(e) = bridge.apply_local(LocalChange::Pref(field)).await {
                    tracing::warn!("apply_local pref: {e}");
                }
            }
        };
        if let Some(v) = patch.display_name {
            push(PrefField::DisplayName(v)).await;
        }
        if let Some(v) = patch.time_zone {
            push(PrefField::TimeZone(v)).await;
        }
        if let Some(v) = patch.historic_gpa {
            push(PrefField::HistoricGpa(v)).await;
        }
        if let Some(v) = patch.historic_units {
            push(PrefField::HistoricUnits(v)).await;
        }
        if let Some(v) = patch.default_semester {
            push(PrefField::DefaultSemester(v)).await;
        }
        if let Some(v) = patch.calendar_show_empty_days {
            push(PrefField::CalendarShowEmptyDays(v)).await;
        }
    }

    get_prefs(state).await
}
