use crate::startup;
use serde::Serialize;

/// What went wrong at launch. Deliberately takes no `AppStateArg`: the state is
/// exactly what fails to build when setup breaks, so a command that required it
/// could never report the failure.
#[derive(Serialize, Default)]
pub struct StartupError {
    /// Setup failed on *this* launch; the app is running degraded.
    pub setup: Option<String>,
    /// The previous launch died before the UI existed. Read once, then cleared.
    pub previous_panic: Option<String>,
}

#[tauri::command]
pub fn startup_error() -> StartupError {
    StartupError {
        setup: startup::setup_error(),
        previous_panic: startup::take_previous_panic(),
    }
}
