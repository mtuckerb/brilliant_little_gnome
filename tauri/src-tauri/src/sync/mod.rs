// Sync orchestration. Submodules handle each Brightspace data type.
// All real per-service logic is stubbed for the initial scaffold.

pub mod background;

use crate::error::Result;
use crate::state::AppState;
use std::sync::Arc;

pub async fn sync_all(state: Arc<AppState>) -> Result<()> {
    {
        let mut st = state.sync_status.write();
        st.status = crate::models::SyncState::Syncing;
        st.current_task = Some("starting".into());
        st.progress = 0.0;
    }
    state.events.sync_progress("starting", 0.0);

    // TODO: actual sync — courses, grades, assignments, content, discussions, notifications.
    // Stub just marks idle so UI can render.
    {
        let mut st = state.sync_status.write();
        st.status = crate::models::SyncState::Idle;
        st.current_task = None;
        st.progress = 1.0;
        st.last_sync_at = Some(chrono::Utc::now().to_rfc3339());
    }
    state.events.sync_done();
    Ok(())
}

pub async fn sync_course(state: Arc<AppState>, course_id: &str) -> Result<()> {
    state.events.sync_progress(&format!("course:{}", course_id), 0.0);
    // TODO: per-course sync.
    state.events.course_updated(course_id);
    state.events.sync_done();
    Ok(())
}
