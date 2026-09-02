//! Launch-failure breadcrumbs.
//!
//! Tauri panics when the `setup` hook returns an `Err` (app.rs: `panic!("Failed
//! to setup app: {e}")`), which on iOS is a SIGABRT before any window exists.
//! The crash report Apple collects carries the stack but *not* the panic
//! message, so a launch failure in the field is invisible: TestFlight shows a
//! dozen frames of Rust panic machinery and nothing about what actually broke.
//!
//! Two mechanisms fix that:
//!
//! 1. A panic hook writes the message + location to a file under the app's tmp
//!    directory. On iOS that lives inside the app container, so it survives the
//!    crash and is still there on the next launch.
//! 2. Setup failures are recorded rather than propagated, so the app starts in
//!    a degraded state and can *show* the error instead of aborting.
//!
//! The frontend reads both through the `startup_error` command.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Set when the setup hook fails. `OnceLock` because only the first failure of
/// a launch is interesting — later ones are usually its consequences.
static SETUP_ERROR: OnceLock<String> = OnceLock::new();

/// Breadcrumb from a *previous* launch that died before the UI existed.
/// On iOS `temp_dir()` resolves inside the app container, so this outlives the
/// crash without needing the app-data dir (which is itself a suspect when
/// startup fails).
fn panic_log_path() -> PathBuf {
    std::env::temp_dir().join("brilliant-last-panic.log")
}

/// Record a panic to disk on the way down. Chained after the default hook so
/// the usual stderr/console output is unchanged.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        // `payload_as_str` is still unstable, so recover the message the long way.
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic with a non-string payload".to_string());
        let _ = std::fs::write(panic_log_path(), format!("{message}\n\nat {location}"));
        previous(info);
    }));
}

/// Remember why setup failed. Called instead of propagating the error, so the
/// app boots far enough to tell the user.
pub fn record_setup_error(error: impl std::fmt::Display) {
    let message = error.to_string();
    tracing::error!("setup failed (continuing in degraded mode): {message}");
    let _ = SETUP_ERROR.set(message);
}

/// This launch's setup failure, if any.
pub fn setup_error() -> Option<String> {
    SETUP_ERROR.get().cloned()
}

/// The panic that killed the *previous* launch, consumed so it is reported
/// once. Returns `None` on a clean run.
pub fn take_previous_panic() -> Option<String> {
    let path = panic_log_path();
    let contents = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let trimmed = contents.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}
