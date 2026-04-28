// Sync orchestration. Each submodule handles a single Brightspace data type.
// Per-service errors are logged but don't abort the whole sync — mirrors the
// Ruby app's per-course rescue.

pub mod assignments;
pub mod background;
pub mod content;
pub mod courses;
pub mod discussions;
pub mod grades;
pub mod notifications;
pub mod psy220;

use crate::error::Result;
use crate::models::SyncState;
use crate::state::AppState;
use std::collections::HashSet;
use std::sync::Arc;

/// Skip a course's deep sync if its `last_synced_at` is younger than this and
/// `force` is false. Matches the HTTP-cache TTL in `client::FRESH_CACHE_SECONDS`,
/// so a skip is only ever taken when the cached payloads would have been served
/// anyway — but we also avoid the DB upsert pass entirely.
const COURSE_SYNC_TTL_SECS: i64 = 600;

/// Full sync: enrollments → for each *active* course (content, assignments,
/// grades, discussions) → notifications.
///
/// `force` bypasses the per-course freshness probe; the upstream HTTP cache in
/// `client::do_get` is also told to refresh when `force == true`.
pub async fn sync_all(state: Arc<AppState>, force: bool) -> Result<()> {
    if !state.client.is_configured() {
        return Err(crate::error::AppError::Unauthenticated);
    }

    set_status(&state, SyncState::Syncing, "starting", 0.0);
    state.events.sync_progress("starting", 0.0);

    // 1) Refresh course list.
    let course_list = match courses::sync_enrollments(&state).await {
        Ok(list) => list,
        Err(e) => {
            tracing::error!("enrollments sync failed: {}", e);
            set_error(&state, &e.to_string());
            state.events.sync_error(&e.to_string());
            return Err(e);
        }
    };

    // 2) Drop archived/dropped courses — they have local data but we don't
    //    re-pull them every sync. Restoring a course flips status back to
    //    'active' and the next sync will pick it up.
    let active_ids = active_course_ids(&state, &course_list).await?;

    let total = (active_ids.len() as f64).max(1.0) + 1.0;

    // 3) Per-course sync.
    for (idx, course_id) in active_ids.iter().enumerate() {
        let progress = (idx as f64) / total;
        set_status(&state, SyncState::Syncing, &format!("course {}", course_id), progress);
        state.events.sync_progress(&format!("course:{}", course_id), progress);

        if !force && course_recently_synced(&state, course_id).await {
            tracing::debug!("[{}] skip: synced within TTL", course_id);
            continue;
        }

        if let Err(e) = content::sync(&state, course_id).await {
            tracing::warn!("[{}] content sync failed: {}", course_id, e);
        }
        if let Err(e) = assignments::sync(&state, course_id).await {
            tracing::warn!("[{}] assignment sync failed: {}", course_id, e);
        }
        if let Err(e) = grades::sync(&state, course_id).await {
            tracing::warn!("[{}] grade sync failed: {}", course_id, e);
        }
        if let Err(e) = discussions::sync(&state, course_id).await {
            tracing::warn!("[{}] discussion sync failed: {}", course_id, e);
        }
        // PSY-220 scraper: only runs for course 446900, no-op otherwise.
        if let Err(e) = psy220::sync(&state, course_id).await {
            tracing::warn!("[{}] psy220 scraper failed: {}", course_id, e);
        }

        mark_course_synced(&state, course_id).await;
        state.events.course_updated(course_id);
    }

    // 4) Notifications (global feed/alerts).
    set_status(&state, SyncState::Syncing, "notifications", (active_ids.len() as f64) / total);
    if let Err(e) = notifications::sync(&state).await {
        tracing::warn!("notifications sync failed: {}", e);
    }

    // Done.
    {
        let mut st = state.sync_status.write();
        st.status = SyncState::Idle;
        st.current_task = None;
        st.progress = 1.0;
        st.last_sync_at = Some(chrono::Utc::now().to_rfc3339());
    }
    state.events.sync_done();
    Ok(())
}

pub async fn sync_course(state: Arc<AppState>, course_id: &str) -> Result<()> {
    set_status(&state, SyncState::Syncing, &format!("course {}", course_id), 0.0);
    state.events.sync_progress(&format!("course:{}", course_id), 0.0);

    if let Err(e) = content::sync(&state, course_id).await {
        tracing::warn!("[{}] content sync failed: {}", course_id, e);
    }
    if let Err(e) = assignments::sync(&state, course_id).await {
        tracing::warn!("[{}] assignment sync failed: {}", course_id, e);
    }
    if let Err(e) = grades::sync(&state, course_id).await {
        tracing::warn!("[{}] grade sync failed: {}", course_id, e);
    }
    if let Err(e) = discussions::sync(&state, course_id).await {
        tracing::warn!("[{}] discussion sync failed: {}", course_id, e);
    }
    if let Err(e) = psy220::sync(&state, course_id).await {
        tracing::warn!("[{}] psy220 scraper failed: {}", course_id, e);
    }

    mark_course_synced(&state, course_id).await;

    {
        let mut st = state.sync_status.write();
        st.status = SyncState::Idle;
        st.current_task = None;
        st.progress = 1.0;
        st.last_sync_at = Some(chrono::Utc::now().to_rfc3339());
    }
    state.events.course_updated(course_id);
    state.events.sync_done();
    Ok(())
}

fn set_status(state: &AppState, status: SyncState, task: &str, progress: f64) {
    let mut st = state.sync_status.write();
    st.status = status;
    st.current_task = Some(task.to_string());
    st.progress = progress;
}

fn set_error(state: &AppState, msg: &str) {
    let mut st = state.sync_status.write();
    st.status = SyncState::Error;
    st.current_task = Some(msg.to_string());
}

/// Intersect the just-synced enrollment IDs with the local DB's active set.
/// Courses missing from the DB row (shouldn't happen — enrollments upsert just
/// ran) are treated as active so we don't silently drop them.
async fn active_course_ids(state: &AppState, all_ids: &[String]) -> Result<Vec<String>> {
    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT org_unit_id, status FROM courses")
            .fetch_all(&state.pool)
            .await?;
    let inactive: HashSet<&str> = rows
        .iter()
        .filter_map(|(id, status)| {
            let s = status.as_deref().unwrap_or("active");
            if s == "dropped" || s == "archived" {
                Some(id.as_str())
            } else {
                None
            }
        })
        .collect();
    Ok(all_ids
        .iter()
        .filter(|id| !inactive.contains(id.as_str()))
        .cloned()
        .collect())
}

async fn course_recently_synced(state: &AppState, course_id: &str) -> bool {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT last_synced_at FROM courses WHERE org_unit_id = ?")
            .bind(course_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let Some((Some(ts),)) = row else { return false };
    age_seconds(&ts).map_or(false, |age| age >= 0 && age < COURSE_SYNC_TTL_SECS)
}

async fn mark_course_synced(state: &AppState, course_id: &str) {
    if let Err(e) = sqlx::query(
        "UPDATE courses SET last_synced_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?",
    )
    .bind(course_id)
    .execute(&state.pool)
    .await
    {
        tracing::warn!("[{}] failed to record last_synced_at: {}", course_id, e);
    }
}

fn age_seconds(updated_at: &str) -> Option<i64> {
    use chrono::{DateTime, NaiveDateTime, Utc};
    let dt: Option<DateTime<Utc>> = DateTime::parse_from_rfc3339(updated_at)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            NaiveDateTime::parse_from_str(updated_at, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|n| n.and_utc())
        });
    dt.map(|t| (Utc::now() - t).num_seconds())
}
