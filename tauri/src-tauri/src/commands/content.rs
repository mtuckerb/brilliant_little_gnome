use super::AppStateArg;
use crate::error::Result;
use crate::models::{ContentItem, ContentModule};

#[tauri::command]
pub async fn list_modules(state: AppStateArg<'_>, course_id: String) -> Result<Vec<ContentModule>> {
    let rows = sqlx::query_as::<_, ContentModule>(
        "SELECT id, course_id, brightspace_id, title, description, sort_order, parent_id
         FROM content_modules WHERE course_id = ? ORDER BY parent_id IS NOT NULL, sort_order, title",
    )
    .bind(&course_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

#[tauri::command]
pub async fn list_items(state: AppStateArg<'_>, module_id: String) -> Result<Vec<ContentItem>> {
    let rows = sqlx::query_as::<_, ContentItem>(
        "SELECT id, module_id, brightspace_id, title, item_type, url, is_hidden, sort_order
         FROM content_items WHERE module_id = ? ORDER BY sort_order, title",
    )
    .bind(&module_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}
