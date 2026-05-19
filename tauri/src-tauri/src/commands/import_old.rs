//! One-shot importer for the old Sinatra/Rails Brilliant database.
//!
//! Opens the old SQLite read-only and merges into the current Tauri DB.
//! Only touches fields the user owns — Brightspace-sourced data stays
//! authoritative and is re-pulled on the next sync.
//!
//! Merge keys:
//!   - courses        : org_unit_id
//!   - assignments    : (course_id, brightspace_id) — synthetic rows
//!                       (brightspace_id LIKE 'syn_%') always inserted
//!                       fresh
//!   - notifications  : (course_id, external_id) via INSERT OR IGNORE so
//!                       historical announcements round-trip without
//!                       overwriting locally-read state
//!   - user_preferences : the singleton row, overlay-only

use super::AppStateArg;
use crate::error::Result;
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Default, Serialize)]
pub struct ImportSummary {
    pub courses_updated: u64,
    pub assignments_updated: u64,
    pub synthetic_assignments_inserted: u64,
    pub notifications_inserted: u64,
    pub prefs_overlaid: bool,
    pub old_db_path: String,
}

fn default_old_db_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join("Library/Application Support/brilliant/db/production.sqlite3"))
}

async fn open_old_db(path: &PathBuf) -> Result<Pool<Sqlite>> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .map_err(|e| crate::error::AppError::Other(format!("old db: {}", e)))?
        .read_only(true);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .map_err(|e| crate::error::AppError::Other(format!("old db open: {}", e)))
}

#[tauri::command]
pub async fn import_from_old_brilliant(
    state: AppStateArg<'_>,
    db_path: Option<String>,
) -> Result<ImportSummary> {
    let path = db_path
        .map(PathBuf::from)
        .or_else(default_old_db_path)
        .ok_or_else(|| crate::error::AppError::Other("Cannot determine home dir".into()))?;
    if !path.exists() {
        return Err(crate::error::AppError::Other(format!(
            "Old Brilliant DB not found at {}",
            path.display()
        )));
    }
    tracing::info!("import: reading {}", path.display());
    let old = open_old_db(&path).await?;
    let mut summary = ImportSummary {
        old_db_path: path.display().to_string(),
        ..Default::default()
    };

    summary.courses_updated = import_courses(&old, &state.pool).await?;
    let (a_updated, a_inserted) = import_assignments(&old, &state.pool).await?;
    summary.assignments_updated = a_updated;
    summary.synthetic_assignments_inserted = a_inserted;
    summary.notifications_inserted = import_notifications(&old, &state.pool).await?;
    summary.prefs_overlaid = import_user_prefs(&old, &state.pool).await?;

    Ok(summary)
}

async fn import_courses(old: &Pool<Sqlite>, new: &Pool<Sqlite>) -> Result<u64> {
    // Pull every old course row that has the user-owned fields we care
    // about. Brightspace re-sync will refill name/code/banner etc.
    let rows: Vec<(
        String,           // org_unit_id
        Option<i64>,      // is_pinned
        Option<String>,   // custom_color
        Option<f64>,      // target_grade
        Option<i64>,      // units
        Option<i64>,      // end_of_week_day
        Option<i64>,      // sort_order
        Option<String>,   // last_accessed_at
    )> = sqlx::query_as(
        "SELECT org_unit_id, is_pinned, custom_color, target_grade, units, end_of_week_day, sort_order, last_accessed_at FROM courses WHERE org_unit_id IS NOT NULL",
    )
    .fetch_all(old)
    .await
    .map_err(|e| crate::error::AppError::Other(format!("old courses: {}", e)))?;

    let mut updated = 0u64;
    for (org_unit_id, is_pinned, color, target, units, eow, sort_order, last_accessed_at) in rows {
        // Only UPDATE — don't INSERT brand-new courses, because they'd
        // have no Brightspace metadata. Next enrollment sync handles that.
        let res = sqlx::query(
            "UPDATE courses SET \
               is_pinned        = COALESCE(?, is_pinned), \
               custom_color     = COALESCE(?, custom_color), \
               target_grade     = COALESCE(?, target_grade), \
               units            = COALESCE(?, units), \
               end_of_week_day  = COALESCE(?, end_of_week_day), \
               sort_order       = COALESCE(?, sort_order), \
               last_accessed_at = COALESCE(?, last_accessed_at), \
               updated_at       = CURRENT_TIMESTAMP \
             WHERE org_unit_id = ?",
        )
        .bind(is_pinned)
        .bind(color)
        .bind(target)
        .bind(units.map(|u| u as f64))
        .bind(eow)
        .bind(sort_order)
        .bind(last_accessed_at)
        .bind(&org_unit_id)
        .execute(new)
        .await
        .map_err(|e| crate::error::AppError::Other(format!("new courses update: {}", e)))?;
        updated += res.rows_affected();
    }
    Ok(updated)
}

async fn import_assignments(old: &Pool<Sqlite>, new: &Pool<Sqlite>) -> Result<(u64, u64)> {
    let rows: Vec<(
        String,         // course_id
        String,         // brightspace_id
        Option<String>, // name
        Option<String>, // due_date
        Option<String>, // description
        Option<i64>,    // completed
        Option<String>, // completed_at
        Option<String>, // external_url
        Option<i64>,    // synthetic
        Option<i64>,    // optional
        Option<i64>,    // manually_edited
        Option<String>, // manually_edited_at
    )> = sqlx::query_as(
        "SELECT course_id, brightspace_id, name, due_date, description, completed, completed_at, external_url, synthetic, optional, manually_edited, manually_edited_at FROM assignments WHERE course_id IS NOT NULL AND brightspace_id IS NOT NULL",
    )
    .fetch_all(old)
    .await
    .map_err(|e| crate::error::AppError::Other(format!("old assignments: {}", e)))?;

    let mut updated = 0u64;
    let mut inserted = 0u64;
    for (course_id, bid, name, due, desc, completed, completed_at, ext_url, synthetic, optional, m_edit, m_edit_at) in rows {
        let is_synthetic = synthetic.unwrap_or(0) != 0 || bid.starts_with("syn_");
        if is_synthetic {
            // Synthetic = full row. Insert if missing; update if present.
            let res = sqlx::query(
                "INSERT INTO assignments (course_id, brightspace_id, name, due_date, description, is_graded, assignment_type, completed, completed_at, synthetic, optional, manually_edited, manually_edited_at, external_url, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, 0, 'synthetic', ?, ?, 1, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
                 ON CONFLICT(course_id, brightspace_id) DO UPDATE SET \
                   name = excluded.name, due_date = excluded.due_date, description = excluded.description, \
                   completed = excluded.completed, completed_at = excluded.completed_at, optional = excluded.optional, \
                   external_url = excluded.external_url, updated_at = CURRENT_TIMESTAMP",
            )
            .bind(&course_id).bind(&bid).bind(name).bind(due).bind(desc)
            .bind(completed.unwrap_or(0))
            .bind(completed_at)
            .bind(optional.unwrap_or(0))
            .bind(m_edit.unwrap_or(0))
            .bind(m_edit_at)
            .bind(ext_url)
            .execute(new).await
            .map_err(|e| crate::error::AppError::Other(format!("synthetic insert: {}", e)))?;
            inserted += res.rows_affected();
        } else {
            // Non-synthetic = overlay user fields only; let the next
            // Brightspace sync own name / due / description.
            let res = sqlx::query(
                "UPDATE assignments SET \
                   completed         = COALESCE(?, completed), \
                   completed_at      = COALESCE(?, completed_at), \
                   optional          = COALESCE(?, optional), \
                   manually_edited   = COALESCE(?, manually_edited), \
                   manually_edited_at = COALESCE(?, manually_edited_at), \
                   external_url      = COALESCE(?, external_url), \
                   updated_at        = CURRENT_TIMESTAMP \
                 WHERE course_id = ? AND brightspace_id = ?",
            )
            .bind(completed).bind(completed_at).bind(optional)
            .bind(m_edit).bind(m_edit_at).bind(ext_url)
            .bind(&course_id).bind(&bid)
            .execute(new).await
            .map_err(|e| crate::error::AppError::Other(format!("assignment overlay: {}", e)))?;
            updated += res.rows_affected();
        }
    }
    Ok((updated, inserted))
}

async fn import_notifications(old: &Pool<Sqlite>, new: &Pool<Sqlite>) -> Result<u64> {
    // INSERT OR IGNORE on (course_id, external_id) so we get historical
    // announcements that have aged out of Brightspace's recent feed, but
    // don't overwrite locally-changed read state.
    let rows: Vec<(
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<i64>,
    )> = sqlx::query_as(
        "SELECT external_id, notification_type, title, body, date, course_id, course_name, urgency, is_personal, url, is_read FROM notifications",
    )
    .fetch_all(old)
    .await
    .map_err(|e| crate::error::AppError::Other(format!("old notifications: {}", e)))?;

    let mut inserted = 0u64;
    for (ext_id, ntype, title, body, date, course_id, course_name, urgency, is_personal, url, is_read) in rows {
        let Some(ext_id) = ext_id else { continue };
        let res = sqlx::query(
            "INSERT OR IGNORE INTO notifications (external_id, notification_type, title, body, date, course_id, course_name, urgency, is_personal, url, is_read, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(ext_id)
        .bind(ntype.unwrap_or_else(|| "announcement".to_string()))
        .bind(title.unwrap_or_default())
        .bind(body)
        .bind(date)
        .bind(course_id)
        .bind(course_name)
        .bind(urgency.unwrap_or(1))
        .bind(is_personal.unwrap_or(0))
        .bind(url)
        .bind(is_read.unwrap_or(0))
        .execute(new).await
        .map_err(|e| crate::error::AppError::Other(format!("notification insert: {}", e)))?;
        inserted += res.rows_affected();
    }
    Ok(inserted)
}

async fn import_user_prefs(old: &Pool<Sqlite>, new: &Pool<Sqlite>) -> Result<bool> {
    let row: Option<(
        Option<String>,
        Option<String>,
        Option<f64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT display_name, time_zone, historic_gpa, historic_units, default_semester, semester_colors, collapsed_topics FROM user_preferences LIMIT 1",
    )
    .fetch_optional(old)
    .await
    .map_err(|e| crate::error::AppError::Other(format!("old prefs: {}", e)))?;
    let Some((dn, tz, gpa, units, sem, colors, topics)) = row else {
        return Ok(false);
    };
    sqlx::query(
        "UPDATE user_preferences SET \
           display_name      = COALESCE(?, display_name), \
           time_zone         = COALESCE(?, time_zone), \
           historic_gpa      = COALESCE(?, historic_gpa), \
           historic_units    = COALESCE(?, historic_units), \
           default_semester  = COALESCE(?, default_semester), \
           semester_colors   = COALESCE(?, semester_colors), \
           collapsed_topics  = COALESCE(?, collapsed_topics), \
           updated_at        = CURRENT_TIMESTAMP",
    )
    .bind(dn)
    .bind(tz)
    .bind(gpa)
    .bind(units.map(|u| u as f64))
    .bind(sem)
    .bind(colors)
    .bind(topics)
    .execute(new)
    .await
    .map_err(|e| crate::error::AppError::Other(format!("new prefs overlay: {}", e)))?;
    Ok(true)
}
