use super::AppStateArg;
use crate::error::Result;
use crate::models::Course;

#[tauri::command]
pub async fn list_courses(state: AppStateArg<'_>) -> Result<Vec<Course>> {
    let rows = sqlx::query_as::<_, Course>(
        "SELECT org_unit_id, name, custom_name, code, semester, is_pinned, custom_color, banner_url, units, target_grade, status, sort_order, end_of_week_day, last_accessed_at FROM courses ORDER BY is_pinned DESC, sort_order ASC, COALESCE(custom_name, name) ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

#[tauri::command]
pub async fn get_course(state: AppStateArg<'_>, id: String) -> Result<Course> {
    let course = sqlx::query_as::<_, Course>(
        "SELECT org_unit_id, name, custom_name, code, semester, is_pinned, custom_color, banner_url, units, target_grade, status, sort_order, end_of_week_day, last_accessed_at FROM courses WHERE org_unit_id = ?",
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

fn extract_course_code(full_name: &str) -> Option<String> {
    let code_re = regex::Regex::new(r"(?i)([A-Z]{2,4})\s*-?\s*(\d{3,4})").ok()?;
    let captures = code_re.captures(full_name)?;
    let prefix = captures.get(1)?.as_str().to_uppercase();
    let number = captures.get(2)?.as_str();
    Some(format!("{}-{}", prefix, number))
}

#[tauri::command]
pub async fn update_course_name(state: AppStateArg<'_>, id: String, name: String) -> Result<Course> {
    let trimmed = name.trim().to_string();
    let custom_name = if trimmed.is_empty() { None } else { Some(trimmed) };
    let previous_code: Option<String> = sqlx::query_scalar("SELECT code FROM courses WHERE org_unit_id = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    let fallback_name: Option<String> = if custom_name.is_none() {
        sqlx::query_scalar("SELECT name FROM courses WHERE org_unit_id = ?")
            .bind(&id)
            .fetch_one(&state.pool)
            .await?
    } else {
        None
    };
    let code_source = custom_name.as_deref().or(fallback_name.as_deref());
    let extracted_code = code_source.and_then(extract_course_code);
    let next_code = extracted_code.or(previous_code);

    sqlx::query("UPDATE courses SET custom_name = ?, code = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(&custom_name)
        .bind(&next_code)
        .bind(&id)
        .execute(&state.pool)
        .await?;

    get_course(state, id).await
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
