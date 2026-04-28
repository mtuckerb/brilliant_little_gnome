// Crate root. Wires up the Tauri app, DB, and shared state.

pub mod auth;
pub mod client;
pub mod commands;
pub mod db;
pub mod error;
pub mod events;
pub mod models;
pub mod rest_api;
pub mod state;
pub mod sync;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,brilliant_tauri_lib=debug")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                let state = AppState::initialize(handle.clone()).await?;
                handle.manage(Arc::new(state));
                anyhow::Ok(())
            })?;

            // Kick off background sync loop after init
            let handle2 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = handle2.try_state::<Arc<AppState>>() {
                    sync::background::run_periodic_loop(state.inner().clone(), handle2.clone()).await;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Auth
            commands::auth::auth_status,
            commands::auth::setup_cookies,
            commands::auth::clear_auth,
            commands::auth::open_login_window,
            // Prefs
            commands::prefs::get_prefs,
            commands::prefs::update_prefs,
            // Courses
            commands::courses::list_courses,
            commands::courses::get_course,
            commands::courses::reorder_courses,
            commands::courses::update_course_color,
            commands::courses::update_course_units,
            commands::courses::update_course_target_grade,
            commands::courses::drop_course,
            commands::courses::refresh_course,
            // Grades
            commands::grades::grades_summary,
            commands::grades::toggle_grade_hidden,
            commands::grades::toggle_grade_ungraded,
            commands::grades::toggle_grade_extra_credit,
            commands::grades::set_expected_score,
            // Assignments
            commands::assignments::list_assignments,
            commands::assignments::toggle_assignment_complete,
            commands::assignments::toggle_assignment_optional,
            commands::assignments::update_assignment_due_date,
            // Notifications
            commands::notifications::list_notifications,
            commands::notifications::mark_notification_read,
            commands::notifications::mark_all_notifications_read,
            // Sync
            commands::sync::sync_status,
            commands::sync::sync_all,
            commands::sync::sync_course,
            // Content
            commands::content::list_modules,
            commands::content::list_items,
            // Discussions
            commands::discussions::list_forums,
            commands::discussions::list_topics,
            // REST API
            commands::rest::rest_api_start,
            commands::rest::rest_api_stop,
            commands::rest::rest_api_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
