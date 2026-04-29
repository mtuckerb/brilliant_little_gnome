use super::AppStateArg;
use crate::error::{AppError, Result};
use crate::models::Assignment;
use rand::RngCore;

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

/// Create a user-defined ("synthetic") assignment that doesn't exist in
/// Brightspace. Mirrors the original Sinatra app's `syn_<hex>` ID convention so
/// downstream sync can recognize them and never overwrite them from the API.
#[tauri::command]
pub async fn create_synthetic_assignment(
    state: AppStateArg<'_>,
    course_id: String,
    name: String,
    due_date: Option<String>,
    description: Option<String>,
) -> Result<Assignment> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Other("name cannot be empty".to_string()));
    }
    let bid = synthetic_id();
    let row: Assignment = sqlx::query_as::<_, Assignment>(
        "INSERT INTO assignments (course_id, brightspace_id, name, due_date, description, is_graded, assignment_type, completed, synthetic, optional, manually_edited, manually_edited_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 0, 'synthetic', 0, 1, 0, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         RETURNING id, course_id, brightspace_id, name, due_date, description, is_graded, grade_item_id, assignment_type, completed, completed_at, synthetic, optional, external_url",
    )
    .bind(&course_id)
    .bind(&bid)
    .bind(trimmed)
    .bind(due_date)
    .bind(description)
    .fetch_one(&state.pool)
    .await?;
    Ok(row)
}

/// Hard-delete an assignment row. Synthetic-only — refuse for API-sourced rows
/// because the next sync would just resurrect them, which would surprise users.
#[tauri::command]
pub async fn delete_assignment(state: AppStateArg<'_>, id: i64) -> Result<()> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT synthetic FROM assignments WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    let Some((synthetic,)) = row else {
        return Err(AppError::Other(format!("assignment {} not found", id)));
    };
    if synthetic == 0 {
        return Err(AppError::Other(
            "only synthetic assignments can be deleted; toggle complete instead".to_string(),
        ));
    }
    sqlx::query("DELETE FROM assignments WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

fn synthetic_id() -> String {
    let mut buf = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut buf);
    let hex: String = buf.iter().map(|b| format!("{:02x}", b)).collect();
    format!("syn_{}", hex)
}
