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
    Ok(())
}

#[tauri::command]
pub async fn mark_all_notifications_read(state: AppStateArg<'_>) -> Result<()> {
    sqlx::query("UPDATE notifications SET is_read = 1, updated_at = CURRENT_TIMESTAMP WHERE is_read = 0")
        .execute(&state.pool).await?;
    state.events.notifications_updated();
    Ok(())
}
