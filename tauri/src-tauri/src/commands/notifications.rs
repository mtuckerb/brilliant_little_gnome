use super::AppStateArg;
use crate::error::Result;
use crate::models::Notification;

#[tauri::command]
pub async fn list_notifications(state: AppStateArg<'_>, unread_only: Option<bool>) -> Result<Vec<Notification>> {
    let rows = if unread_only.unwrap_or(false) {
        sqlx::query_as::<_, Notification>(
            "SELECT id, external_id, notification_type, title, body, date, course_id, course_name, urgency, is_personal, is_read, url FROM notifications WHERE is_read = 0 ORDER BY date DESC NULLS LAST",
        )
        .fetch_all(&state.pool).await?
    } else {
        sqlx::query_as::<_, Notification>(
            "SELECT id, external_id, notification_type, title, body, date, course_id, course_name, urgency, is_personal, is_read, url FROM notifications ORDER BY date DESC NULLS LAST LIMIT 200",
        )
        .fetch_all(&state.pool).await?
    };
    Ok(rows)
}

#[tauri::command]
pub async fn mark_notification_read(state: AppStateArg<'_>, id: i64) -> Result<()> {
    sqlx::query("UPDATE notifications SET is_read = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id).execute(&state.pool).await?;
    state.events.notifications_updated();
    #[cfg(feature = "p2p")]
    if let Some(external_id) = read_external_id(&state, id).await {
        push_notification_read(&state, external_id, true).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn mark_all_notifications_read(state: AppStateArg<'_>) -> Result<()> {
    // Capture the set of unread external_ids BEFORE flipping them, so
    // we can mirror each one into Loro after the SQL UPDATE. Doing
    // the SELECT after would return zero rows.
    #[cfg(feature = "p2p")]
    let unread: Vec<String> = sqlx::query_scalar::<_, String>(
        "SELECT external_id FROM notifications WHERE is_read = 0",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    sqlx::query("UPDATE notifications SET is_read = 1, updated_at = CURRENT_TIMESTAMP WHERE is_read = 0")
        .execute(&state.pool).await?;
    state.events.notifications_updated();

    #[cfg(feature = "p2p")]
    {
        for external_id in unread {
            push_notification_read(&state, external_id, true).await;
        }
    }
    Ok(())
}

// ---- p2p sync helpers (feature-gated) ------------------------------------

#[cfg(feature = "p2p")]
async fn read_external_id(state: &AppStateArg<'_>, id: i64) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT external_id FROM notifications WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()
}

#[cfg(feature = "p2p")]
async fn push_notification_read(state: &AppStateArg<'_>, external_id: String, read: bool) {
    if let Some(engine) = state.sync_engine() {
        if let Err(e) = engine
            .bridge()
            .apply_local(crate::p2p::bridge::LocalChange::NotificationRead {
                external_id: external_id.clone(),
                read,
            })
            .await
        {
            tracing::warn!("apply_local notification_read {}: {}", external_id, e);
        }
    }
}
