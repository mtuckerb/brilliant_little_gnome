use super::AppStateArg;
use crate::error::Result;
use crate::models::Assignment;

#[tauri::command]
pub async fn list_assignments(state: AppStateArg<'_>, course_id: Option<String>) -> Result<Vec<Assignment>> {
    let rows = if let Some(cid) = course_id {
        sqlx::query_as::<_, Assignment>(
            "SELECT id, course_id, brightspace_id, name, due_date, description, is_graded, grade_item_id, assignment_type, completed, completed_at, synthetic, optional, external_url FROM assignments WHERE course_id = ? ORDER BY due_date ASC NULLS LAST",
        )
        .bind(cid)
        .fetch_all(&state.pool).await?
    } else {
        sqlx::query_as::<_, Assignment>(
            "SELECT id, course_id, brightspace_id, name, due_date, description, is_graded, grade_item_id, assignment_type, completed, completed_at, synthetic, optional, external_url FROM assignments WHERE completed = 0 ORDER BY due_date ASC NULLS LAST",
        )
        .fetch_all(&state.pool).await?
    };
    Ok(rows)
}

#[tauri::command]
pub async fn toggle_assignment_complete(state: AppStateArg<'_>, id: i64) -> Result<()> {
    sqlx::query("UPDATE assignments SET completed = 1 - completed, completed_at = CASE WHEN completed = 0 THEN CURRENT_TIMESTAMP ELSE NULL END, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id).execute(&state.pool).await?;
    Ok(())
}

#[tauri::command]
pub async fn toggle_assignment_optional(state: AppStateArg<'_>, id: i64) -> Result<()> {
    sqlx::query("UPDATE assignments SET optional = 1 - optional, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id).execute(&state.pool).await?;
    Ok(())
}

#[tauri::command]
pub async fn update_assignment_due_date(state: AppStateArg<'_>, id: i64, due_date: Option<String>) -> Result<()> {
    sqlx::query("UPDATE assignments SET due_date = ?, manually_edited = 1, manually_edited_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(due_date).bind(id).execute(&state.pool).await?;
    Ok(())
}
