use super::AppStateArg;
use crate::error::Result;
use crate::models::Course;
use base64::Engine;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CourseBanner {
    pub data_url: String,
}

#[tauri::command]
pub async fn list_courses(state: AppStateArg<'_>) -> Result<Vec<Course>> {
    let rows = sqlx::query_as::<_, Course>(
        "SELECT org_unit_id, name, custom_name, code, custom_code, custom_room, custom_meeting_days, custom_meeting_time, syllabus_meeting_info, semester, custom_semester, is_pinned, custom_color, banner_url, units, target_grade, status, sort_order, end_of_week_day, last_accessed_at FROM courses ORDER BY is_pinned DESC, sort_order ASC, COALESCE(custom_name, name) ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

#[tauri::command]
pub async fn get_course(state: AppStateArg<'_>, id: String) -> Result<Course> {
    let course = sqlx::query_as::<_, Course>(
        "SELECT org_unit_id, name, custom_name, code, custom_code, custom_room, custom_meeting_days, custom_meeting_time, syllabus_meeting_info, semester, custom_semester, is_pinned, custom_color, banner_url, units, target_grade, status, sort_order, end_of_week_day, last_accessed_at FROM courses WHERE org_unit_id = ?",
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

/// Pin or unpin a course. Brightspace seeds the initial pin state from the
/// user's PinDate, but from then on this is user-controlled in Brilliant (the
/// enrollment sync no longer overwrites it), and the choice mirrors to peers.
#[tauri::command]
pub async fn set_course_pinned(state: AppStateArg<'_>, id: String, pinned: bool) -> Result<()> {
    sqlx::query("UPDATE courses SET is_pinned = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(pinned as i64)
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
                    field: CourseField::IsPinned(pinned),
                })
                .await
            {
                tracing::warn!("apply_local is_pinned {}: {}", id, e);
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

    #[cfg(feature = "p2p")]
    {
        use crate::p2p::bridge::LocalChange;
        use crate::p2p::doc::CourseField;
        if let Some(engine) = state.sync_engine() {
            if let Err(e) = engine
                .bridge()
                .apply_local(LocalChange::Course {
                    id: id.clone(),
                    field: CourseField::CustomName(custom_name.clone()),
                })
                .await
            {
                tracing::warn!("apply_local custom_name {}: {}", id, e);
            }
        }
    }

    get_course(state, id).await
}

#[tauri::command]
pub async fn update_course_semester(state: AppStateArg<'_>, id: String, semester: Option<String>) -> Result<()> {
    // Override for the Brightspace-assigned semester. Empty/whitespace clears
    // the override so the display falls back to whatever Brightspace says.
    let trimmed = semester.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    });
    sqlx::query("UPDATE courses SET custom_semester = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(&trimmed)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn update_course_code(state: AppStateArg<'_>, id: String, code: Option<String>) -> Result<()> {
    // Empty/whitespace → None: clears the override so display falls back to
    // the auto-derived `code` column. This mirrors `update_course_name`'s
    // semantics for `custom_name`.
    let trimmed = code.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    });
    sqlx::query("UPDATE courses SET custom_code = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(&trimmed)
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
                    field: CourseField::CustomCode(trimmed.clone()),
                })
                .await
            {
                tracing::warn!("apply_local custom_code {}: {}", id, e);
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
    state.events.course_updated(&id);
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
pub async fn update_course_room(state: AppStateArg<'_>, id: String, room: Option<String>) -> Result<()> {
    let trimmed = room.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    sqlx::query("UPDATE courses SET custom_room = ?, meeting_sync_pending = meeting_sync_pending | 1, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(&trimmed)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    #[cfg(feature = "p2p")]
    if let Some(engine) = state.sync_engine() {
        engine.bridge().publish_course_meeting_details(&id).await?;
    }
    state.events.course_updated(&id);
    Ok(())
}

// Only small extracted display values cross the wire, never syllabus files.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseMeetingInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub days: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub days_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_source: Option<String>,
}

async fn store_course_meeting_info(pool: &sqlx::SqlitePool, id: &str, info: CourseMeetingInfo, replace: bool) -> Result<(CourseMeetingInfo, bool)> {
    for value in [&info.room, &info.room_source, &info.days, &info.days_source, &info.time, &info.time_source].into_iter().flatten() {
        if value.len() > 1024 { return Err(crate::error::AppError::BadRequest("Course meeting value is too long".into())); }
    }
    let mut tx = pool.begin().await?;
    let stored: Option<String> = sqlx::query_scalar("SELECT syllabus_meeting_info FROM courses WHERE org_unit_id = ?")
        .bind(id).fetch_one(&mut *tx).await?;
    let old = stored.as_deref().and_then(|v| serde_json::from_str::<CourseMeetingInfo>(v).ok());
    let next = if replace { info } else {
        let mut next = old.clone().unwrap_or_default();
        if info.room.is_some() { next.room = info.room; next.room_source = info.room_source; }
        if info.days.is_some() { next.days = info.days; next.days_source = info.days_source; }
        if info.time.is_some() { next.time = info.time; next.time_source = info.time_source; }
        next
    };
    let changed = old.as_ref() != Some(&next);
    if changed {
        sqlx::query("UPDATE courses SET syllabus_meeting_info = ?, meeting_sync_pending = meeting_sync_pending | 8, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
            .bind(serde_json::to_string(&next)?).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok((next, changed))
}

#[tauri::command]
pub async fn cache_course_meeting_info(state: AppStateArg<'_>, id: String, info: CourseMeetingInfo, replace: bool) -> Result<CourseMeetingInfo> {
    let (info, changed) = store_course_meeting_info(&state.pool, &id, info, replace).await?;
    #[cfg(feature = "p2p")]
    if let Some(engine) = state.sync_engine() {
        engine.bridge().publish_course_meeting_details(&id).await?;
    }
    if changed { state.events.course_updated(&id); }
    Ok(info)
}

#[tauri::command]
pub async fn update_course_schedule(state: AppStateArg<'_>, id: String, days: Option<String>, time: Option<String>) -> Result<()> {
    let days = days.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let time = time.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    sqlx::query("UPDATE courses SET custom_meeting_days = ?, custom_meeting_time = ?, meeting_sync_pending = meeting_sync_pending | 6, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(&days).bind(&time).bind(&id).execute(&state.pool).await?;
    #[cfg(feature = "p2p")]
    if let Some(engine) = state.sync_engine() {
        engine.bridge().publish_course_meeting_details(&id).await?;
    }
    state.events.course_updated(&id);
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

/// Permanently delete a course and everything tied to it. Used for
/// onboarding dummies / mis-imports where archiving isn't enough. The next
/// Brightspace enrollment sync will recreate the row if the user is still
/// enrolled, which is desirable when the deletion was a mistake.
#[tauri::command]
pub async fn delete_course(state: AppStateArg<'_>, id: String) -> Result<()> {
    let mut tx = state.pool.begin().await?;
    // Order matters: child rows first, then the course. Foreign keys aren't
    // declared on every table here, but we want them gone regardless.
    sqlx::query("DELETE FROM assignments WHERE course_id = ?").bind(&id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM grades WHERE course_id = ?").bind(&id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM course_roster WHERE course_id = ?").bind(&id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM content_modules WHERE course_id = ?").bind(&id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM content_items WHERE module_id IN (SELECT brightspace_id FROM content_modules WHERE course_id = ?)")
        .bind(&id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM discussion_forums WHERE course_id = ?").bind(&id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM discussion_topics WHERE course_id = ?").bind(&id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM courses WHERE org_unit_id = ?").bind(&id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

#[tauri::command]
pub async fn refresh_course(state: AppStateArg<'_>, id: String) -> Result<()> {
    crate::sync::sync_course(state.inner().clone(), &id).await
}

/// Fetch the course banner image via the authenticated Brightspace client
/// and return it as a base64 data URL. Brightspace banner URLs point at the
/// /d2l/api/.../image endpoint which requires the session cookie — the Tauri
/// webview can't send that, so we proxy through the Rust HTTP client.
/// Returns None when the course has no banner_url, or when the fetch fails
/// (so the UI can fall back to the accent-color block).
#[tauri::command]
pub async fn fetch_course_banner(state: AppStateArg<'_>, id: String) -> Result<Option<CourseBanner>> {
    let url: Option<String> = sqlx::query_scalar("SELECT banner_url FROM courses WHERE org_unit_id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    let Some(url) = url else { return Ok(None) };
    match state.client.fetch_bytes(&url).await {
        Ok((bytes, mime, _filename)) => {
            let mime = mime.unwrap_or_else(|| "image/jpeg".to_string());
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            Ok(Some(CourseBanner { data_url: format!("data:{};base64,{}", mime, b64) }))
        }
        Err(e) => {
            tracing::warn!("banner fetch for {}: {}", id, e);
            Ok(None)
        }
    }
}

#[cfg(test)]
mod meeting_info_tests {
    use super::*;
    #[tokio::test]
    async fn partial_syllabus_reads_preserve_peer_values_and_never_modify_overrides() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO courses (org_unit_id, name, custom_room, custom_meeting_days, custom_meeting_time) VALUES ('a', 'Biology', '305', 'Friday', '2 PM')").execute(&pool).await.unwrap();
        let initial = CourseMeetingInfo { room: Some("214".into()), days: Some("Monday".into()), time: Some("9 AM".into()), ..Default::default() };
        assert!(store_course_meeting_info(&pool, "a", initial.clone(), true).await.unwrap().1);
        assert!(!store_course_meeting_info(&pool, "a", initial, true).await.unwrap().1);
        let partial = CourseMeetingInfo { room: Some("999".into()), ..Default::default() };
        let (stored, _) = store_course_meeting_info(&pool, "a", partial, false).await.unwrap();
        assert_eq!(stored.days.as_deref(), Some("Monday"));
        assert_eq!(stored.time.as_deref(), Some("9 AM"));
        assert_eq!(stored.room.as_deref(), Some("999"));
        let (stored, _) = store_course_meeting_info(&pool, "a", CourseMeetingInfo::default(), true).await.unwrap();
        assert_eq!(stored, CourseMeetingInfo::default());
        let course = sqlx::query_as::<_, crate::models::Course>("SELECT * FROM courses WHERE org_unit_id = 'a'").fetch_one(&pool).await.unwrap();
        assert_eq!(course.custom_room.as_deref(), Some("305"));
        assert_eq!(course.custom_meeting_days.as_deref(), Some("Friday"));
        assert_eq!(course.custom_meeting_time.as_deref(), Some("2 PM"));
    }
}
