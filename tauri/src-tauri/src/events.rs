// EventBus — emits structured events to the React frontend via Tauri's emit API.
// Replaces the SSE channel from the Ruby app.

use crate::models::SyncStatus;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
pub struct EventBus {
    app: AppHandle,
}

impl EventBus {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    pub fn emit<T: Serialize + Clone>(&self, event: &str, payload: T) {
        if let Err(e) = self.app.emit(event, payload) {
            tracing::warn!("event emit failed for {}: {}", event, e);
        }
    }

    pub fn sync_progress(&self, task: &str, progress: f64) {
        self.emit("sync:progress", SyncProgressEvent {
            task: task.to_string(),
            progress,
        });
    }

    pub fn sync_done(&self) {
        self.emit("sync:done", ());
    }

    /// Emit the unified `app-event` the frontend's `onAppEvent` listener
    /// consumes. The React side (App banner, sidebar `CourseRows`, `DueSoon`,
    /// Dashboard) keys course/assignment refreshes off the `syncing → idle`
    /// transition carried here. The legacy `sync:progress`/`sync:done`
    /// channels above are kept for any direct subscribers, but this is the
    /// canonical signal the discriminated-union frontend actually listens for.
    pub fn sync_status_changed(&self, status: &SyncStatus) {
        self.emit(
            "app-event",
            SyncStatusChangedEvent {
                kind: "sync_status_changed",
                status: status.clone(),
            },
        );
    }

    pub fn sync_error(&self, message: &str) {
        self.emit("sync:error", SyncErrorEvent { message: message.to_string() });
    }

    pub fn course_updated(&self, course_id: &str) {
        self.emit("course:updated", CourseUpdatedEvent { course_id: course_id.to_string() });
    }

    pub fn notifications_updated(&self) {
        self.emit("notifications:updated", ());
    }

    pub fn prefs_updated(&self) {
        self.emit("prefs:updated", ());
    }

    pub fn assignments_updated(&self) {
        self.emit("assignments:updated", ());
    }

    pub fn grades_updated(&self) {
        self.emit("grades:updated", ());
    }

    /// Emit a `p2p:warning` event to the React frontend. Used by the
    /// sync engine's checkpoint scheduler when the on-disk snapshot
    /// crosses the 50 MB threshold (T-022). The Settings → Sync
    /// panel renders these via the existing toast provider.
    pub fn p2p_warning(&self, message: &str) {
        self.emit(
            "p2p:warning",
            P2pWarningEvent {
                message: message.to_string(),
            },
        );
    }
}

#[derive(Serialize, Clone)]
struct SyncStatusChangedEvent {
    kind: &'static str,
    status: SyncStatus,
}

#[derive(Serialize, Clone)]
struct P2pWarningEvent {
    message: String,
}

#[derive(Serialize, Clone)]
struct SyncProgressEvent {
    task: String,
    progress: f64,
}

#[derive(Serialize, Clone)]
struct SyncErrorEvent {
    message: String,
}

#[derive(Serialize, Clone)]
struct CourseUpdatedEvent {
    course_id: String,
}
