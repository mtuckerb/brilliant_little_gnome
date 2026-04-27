// Sync orchestration. Each submodule handles a single Brightspace data type.
// Per-service errors are logged but don't abort the whole sync — mirrors the
// Ruby app's per-course rescue.

pub mod assignments;
pub mod background;
pub mod content;
pub mod courses;
pub mod grades;
pub mod notifications;

use crate::error::Result;
use crate::models::SyncState;
use crate::state::AppState;
use std::sync::Arc;

/// Full sync: enrollments → for each course (content, assignments, grades) → notifications.
pub async fn sync_all(state: Arc<AppState>) -> Result<()> {
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

    let total = (course_list.len() as f64).max(1.0) + 1.0;

    // 2) Per-course sync.
    for (idx, course_id) in course_list.iter().enumerate() {
        let progress = (idx as f64) / total;
        set_status(&state, SyncState::Syncing, &format!("course {}", course_id), progress);
        state.events.sync_progress(&format!("course:{}", course_id), progress);

        if let Err(e) = content::sync(&state, course_id).await {
            tracing::warn!("[{}] content sync failed: {}", course_id, e);
        }
        if let Err(e) = assignments::sync(&state, course_id).await {
            tracing::warn!("[{}] assignment sync failed: {}", course_id, e);
        }
        if let Err(e) = grades::sync(&state, course_id).await {
            tracing::warn!("[{}] grade sync failed: {}", course_id, e);
        }
        state.events.course_updated(course_id);
    }

    // 3) Notifications (global feed/alerts).
    set_status(&state, SyncState::Syncing, "notifications", (course_list.len() as f64) / total);
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
