// EventBus — emits structured events to the React frontend via Tauri's emit API.
// Replaces the SSE channel from the Ruby app.

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
