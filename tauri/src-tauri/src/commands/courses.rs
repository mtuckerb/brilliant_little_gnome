use super::AppStateArg;
use crate::error::Result;
use crate::models::Course;

#[tauri::command]
pub async fn list_courses(state: AppStateArg<'_>) -> Result<Vec<Course>> {
    let rows = sqlx::query_as::<_, Course>(
        "SELECT org_unit_id, name, code, semester, is_pinned, custom_color, banner_url, units, target_grade, status, sort_order, end_of_week_day, last_accessed_at FROM courses ORDER BY is_pinned DESC, sort_order ASC, name ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

#[tauri::command]
pub async fn get_course(state: AppStateArg<'_>, id: String) -> Result<Course> {
    let course = sqlx::query_as::<_, Course>(
        "SELECT org_unit_id, name, code, semester, is_pinned, custom_color, banner_url, units, target_grade, status, sort_order, end_of_week_day, last_accessed_at FROM courses WHERE org_unit_id = ?",
    )
    .bind(&id)
    .fetch_one(&state.pool)
    .await?;
    Ok(course)
}

#[tauri::command]
pub async fn reorder_courses(state: AppStateArg<'_>, ordered_ids: Vec<String>) -> Result<()> {
    let mut tx = state.pool.begin().await?;
    for (i, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE courses SET sort_order = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
            .bind(i as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[tauri::command]
pub async fn update_course_color(state: AppStateArg<'_>, id: String, color: Option<String>) -> Result<()> {
    sqlx::query("UPDATE courses SET custom_color = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(color)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn update_course_units(state: AppStateArg<'_>, id: String, units: Option<f64>) -> Result<()> {
    sqlx::query("UPDATE courses SET units = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(units)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn update_course_target_grade(state: AppStateArg<'_>, id: String, target: Option<f64>) -> Result<()> {
    sqlx::query("UPDATE courses SET target_grade = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(target)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn drop_course(state: AppStateArg<'_>, id: String) -> Result<()> {
    sqlx::query("UPDATE courses SET status = 'dropped', updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn refresh_course(state: AppStateArg<'_>, id: String) -> Result<()> {
    crate::sync::sync_course(state.inner().clone(), &id).await
}
