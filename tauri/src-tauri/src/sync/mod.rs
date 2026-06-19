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
use futures::stream::{self, StreamExt};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Skip a course's deep sync if its `last_synced_at` is younger than this and
/// `force` is false. Matches the HTTP-cache TTL in `client::FRESH_CACHE_SECONDS`,
/// so a skip is only ever taken when the cached payloads would have been served
/// anyway — but we also avoid the DB upsert pass entirely.
const COURSE_SYNC_TTL_SECS: i64 = 600;

/// How many courses to sync in parallel. SQLite is in WAL with an 8-connection
/// pool; 3 concurrent courses × 4 in-course futures ≤ 12 simultaneous queries
/// fits comfortably and keeps the network parallelism dominant over DB lock
/// contention. Bumping this past ~4 starts running into Brightspace's per-IP
/// rate limiting in practice.
const COURSE_CONCURRENCY: usize = 3;

/// Full sync: enrollments → for each *active* course, in parallel batches
/// (content, assignments, grades, discussions concurrently within a course)
/// → notifications.
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
    let enrollment = match courses::sync_enrollments(&state).await {
        Ok(list) => list,
        Err(e) => {
            tracing::error!("enrollments sync failed: {}", e);
            set_error(&state, &e.to_string());
            state.events.sync_error(&e.to_string());
            return Err(e);
        }
    };
    // A newly-available course's announcements predate the global notification
    // high-water mark, so the normal `?since=` delta feed (step 4) would skip
    // them. When any course is new this pass — or the caller forced a sync — do
    // a since-less notification pull so that history backfills. Tying it to
    // `force` lets a manual "Sync" recover announcements for a course that
    // already appeared in an earlier pass (when only its delta window was new).
    let backfill_notifications = force || !enrollment.new_ids.is_empty();
    if backfill_notifications {
        tracing::info!(
            "notification backfill this sync (force={}, new_courses={:?})",
            force, enrollment.new_ids,
        );
    }

    // 2) Drop archived/dropped courses.
    let active_ids = active_course_ids(&state, &enrollment.all_ids).await?;
    let total = (active_ids.len() as f64).max(1.0) + 1.0;
    let completed = Arc::new(AtomicUsize::new(0));

    // 3) Per-course sync, fanned out with a concurrency cap.
    stream::iter(active_ids.iter().cloned())
        .for_each_concurrent(COURSE_CONCURRENCY, |course_id| {
            let state = state.clone();
            let completed = completed.clone();
            async move {
                if !force && course_recently_synced(&state, &course_id).await {
                    tracing::debug!("[{}] skip: synced within TTL", course_id);
                } else {
                    sync_course_data(&state, &course_id).await;
                    mark_course_synced(&state, &course_id).await;
                }

                // Progress is monotonic in completion order, not iteration
                // order, since tasks finish out-of-order under concurrency.
                let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                let progress = (done as f64) / total;
                set_status(&state, SyncState::Syncing, &format!("course {}", course_id), progress);
                state.events.sync_progress(&format!("course:{}", course_id), progress);
                state.events.course_updated(&course_id);
            }
        })
        .await;

    // 4) Notifications (global feed/alerts).
    set_status(&state, SyncState::Syncing, "notifications", (active_ids.len() as f64) / total);
    if let Err(e) = notifications::sync(&state, backfill_notifications).await {
        tracing::warn!("notifications sync failed: {}", e);
    }

    // Done.
    set_done(&state);
    state.events.sync_done();
    Ok(())
}

pub async fn sync_course(state: Arc<AppState>, course_id: &str) -> Result<()> {
    set_status(&state, SyncState::Syncing, &format!("course {}", course_id), 0.0);
    state.events.sync_progress(&format!("course:{}", course_id), 0.0);

    sync_course_data(&state, course_id).await;
    mark_course_synced(&state, course_id).await;

    set_done(&state);
    state.events.course_updated(course_id);
    state.events.sync_done();
    Ok(())
}

/// Run the four independent data-type syncs for a course in parallel, then the
/// PSY-220 scraper. Errors are logged per-service so a failure in one (e.g.
/// discussions disabled by the instructor) doesn't take the others down.
async fn sync_course_data(state: &AppState, course_id: &str) {
    let (c, a, g, d) = tokio::join!(
        content::sync(state, course_id),
        assignments::sync(state, course_id),
        grades::sync(state, course_id),
        discussions::sync(state, course_id),
    );
    if let Err(e) = c { tracing::warn!("[{}] content sync failed: {}", course_id, e); }
    if let Err(e) = a { tracing::warn!("[{}] assignment sync failed: {}", course_id, e); }
    if let Err(e) = g { tracing::warn!("[{}] grade sync failed: {}", course_id, e); }
    if let Err(e) = d { tracing::warn!("[{}] discussion sync failed: {}", course_id, e); }

    // PSY-220 scraper: only runs for course 446900, no-op otherwise. Kept
    // sequential because it scrapes a rendered HTML page that depends on the
    // logged-in cookie state and is intentionally low-priority.
    if let Err(e) = psy220::sync(state, course_id).await {
        tracing::warn!("[{}] psy220 scraper failed: {}", course_id, e);
    }
}

fn set_status(state: &AppState, status: SyncState, task: &str, progress: f64) {
    let snapshot = {
        let mut st = state.sync_status.write();
        st.status = status;
        st.current_task = Some(task.to_string());
        st.progress = progress;
        st.clone()
    };
    state.events.sync_status_changed(&snapshot);
}

fn set_error(state: &AppState, msg: &str) {
    let snapshot = {
        let mut st = state.sync_status.write();
        st.status = SyncState::Error;
        st.current_task = Some(msg.to_string());
        st.clone()
    };
    state.events.sync_status_changed(&snapshot);
}

/// Mark the sync run finished: flip to Idle, stamp `last_sync_at`, and emit the
/// `syncing → idle` transition the frontend keys its post-sync refreshes off.
fn set_done(state: &AppState) {
    let snapshot = {
        let mut st = state.sync_status.write();
        st.status = SyncState::Idle;
        st.current_task = None;
        st.progress = 1.0;
        st.last_sync_at = Some(chrono::Utc::now().to_rfc3339());
        st.clone()
    };
    state.events.sync_status_changed(&snapshot);
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
