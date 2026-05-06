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
    // T-016: mirror each new sort_order into Loro so the user's
    // ordering syncs to other devices. Done after commit so a sync
    // failure can't roll back the SQL — apply_local errors are
    // logged + dropped (sync engine may not be running, and even if
    // it is, a transient Loro write shouldn't fail the user's
    // reorder).
    #[cfg(feature = "p2p")]
    {
        use crate::p2p::bridge::LocalChange;
        use crate::p2p::doc::CourseField;
        if let Some(engine) = state.sync_engine() {
            for (i, id) in ordered_ids.iter().enumerate() {
                if let Err(e) = engine
                    .bridge()
                    .apply_local(LocalChange::Course {
                        id: id.clone(),
                        field: CourseField::SortOrder(Some(i as i64)),
                    })
                    .await
                {
                    tracing::warn!("apply_local sort_order {}: {}", id, e);
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn update_course_color(state: AppStateArg<'_>, id: String, color: Option<String>) -> Result<()> {
    sqlx::query("UPDATE courses SET custom_color = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(&color)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    #[cfg(feature = "p2p")]
    {
        use crate::p2p::bridge::LocalChange;
        use crate::p2p::doc::CourseField;
        if let Some(engine) = state.sync_engine() {
            if let Err(e) = engine
                .bridge()
                .apply_local(LocalChange::Course {
                    id: id.clone(),
                    field: CourseField::CustomColor(color.clone()),
                })
                .await
            {
                tracing::warn!("apply_local custom_color {}: {}", id, e);
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn update_course_units(state: AppStateArg<'_>, id: String, units: Option<f64>) -> Result<()> {
    sqlx::query("UPDATE courses SET units = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(units)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    #[cfg(feature = "p2p")]
    {
        use crate::p2p::bridge::LocalChange;
        use crate::p2p::doc::CourseField;
        if let Some(engine) = state.sync_engine() {
            if let Err(e) = engine
                .bridge()
                .apply_local(LocalChange::Course {
                    id: id.clone(),
                    field: CourseField::Units(units),
                })
                .await
            {
                tracing::warn!("apply_local units {}: {}", id, e);
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn update_course_target_grade(state: AppStateArg<'_>, id: String, target: Option<f64>) -> Result<()> {
    sqlx::query("UPDATE courses SET target_grade = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(target)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    #[cfg(feature = "p2p")]
    {
        use crate::p2p::bridge::LocalChange;
        use crate::p2p::doc::CourseField;
        if let Some(engine) = state.sync_engine() {
            if let Err(e) = engine
                .bridge()
                .apply_local(LocalChange::Course {
                    id: id.clone(),
                    field: CourseField::TargetGrade(target),
                })
                .await
            {
                tracing::warn!("apply_local target_grade {}: {}", id, e);
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn update_course_end_of_week(state: AppStateArg<'_>, id: String, day: i64) -> Result<()> {
    // 0 = Sunday … 6 = Saturday. Reject anything outside that range so we don't
    // store junk that downstream date math will silently mis-handle.
    if !(0..=6).contains(&day) {
        return Err(crate::error::AppError::Other(format!("end_of_week_day out of range: {}", day)));
    }
    sqlx::query("UPDATE courses SET end_of_week_day = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(day)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    #[cfg(feature = "p2p")]
    {
        use crate::p2p::bridge::LocalChange;
        use crate::p2p::doc::CourseField;
        if let Some(engine) = state.sync_engine() {
            if let Err(e) = engine
                .bridge()
                .apply_local(LocalChange::Course {
                    id: id.clone(),
                    field: CourseField::EndOfWeekDay(Some(day)),
                })
                .await
            {
                tracing::warn!("apply_local end_of_week_day {}: {}", id, e);
            }
        }
    }
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
