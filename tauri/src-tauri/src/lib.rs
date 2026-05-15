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

#[cfg(feature = "p2p")]
pub mod p2p;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;

#[cfg(not(debug_assertions))]
fn release_webview_uses_embedded_assets(app: &tauri::App) -> anyhow::Result<()> {
    let Some(main_window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let url = main_window.url()?;
    let is_http = url.scheme() == "http" || url.scheme() == "https";
    let is_tauri_embedded_origin = url.host_str() == Some("tauri.localhost");
    if is_http && !is_tauri_embedded_origin {
        anyhow::bail!(
            "release build refused to start with external WebView origin: {url}. \
             iOS release builds must load embedded assets from frontendDist, not the Vite dev server."
        );
    }

    Ok(())
}
#[cfg(feature = "p2p")]
use tauri::{RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,brilliant_tauri_lib=debug")),
        )
        .init();

    // rustls 0.23 (used by reqwest 0.13 via iroh + tauri) refuses to make
    // TLS connections until a CryptoProvider is installed. Doing it here
    // before any HTTP client is constructed avoids the "No provider set"
    // panic. Idempotent — install_default returns Err if already set.
    let _ = rustls::crypto::ring::default_provider().install_default();

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init());

    // Mobile-only: native QR scanner for the device-pairing flow. Desktop
    // builds rely on the manual-paste fallback in SyncPanel.
    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    }

    builder
        .setup(|app| {
            #[cfg(not(debug_assertions))]
            release_webview_uses_embedded_assets(app)?;

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
            commands::courses::update_course_name,
            commands::courses::update_course_units,
            commands::courses::update_course_target_grade,
            commands::courses::update_course_end_of_week,
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
            commands::assignments::create_synthetic_assignment,
            commands::assignments::delete_assignment,
            commands::assignment_detail::get_assignment_detail,
            commands::downloads::download_topic_file,
            commands::downloads::download_module_archive,
            commands::downloads::download_course_archive,
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
            commands::discussions::list_topic_posts,
            // Overview / syllabus
            commands::overview::get_course_overview,
            commands::overview::fetch_course_overview_attachment,
            // REST API
            commands::rest::rest_api_start,
            commands::rest::rest_api_stop,
            commands::rest::rest_api_status,
            // P2P device-to-device sync (T-014). Gated on the `p2p`
            // feature so the no-network build doesn't expose a UI
            // surface that has no engine to back it.
            #[cfg(feature = "p2p")]
            commands::sync_p2p::p2p_status,
            #[cfg(feature = "p2p")]
            commands::sync_p2p::p2p_enable,
            #[cfg(feature = "p2p")]
            commands::sync_p2p::p2p_disable,
            #[cfg(feature = "p2p")]
            commands::sync_p2p::p2p_pairing_qr,
            #[cfg(feature = "p2p")]
            commands::sync_p2p::p2p_consume_pairing,
            #[cfg(feature = "p2p")]
            commands::sync_p2p::p2p_rotate,
            #[cfg(feature = "p2p")]
            commands::sync_p2p::p2p_storage_stats,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {
            // T-020: shut the sync engine down on graceful close so the
            // final checkpoint lands and the WAL stays minimal. Both
            // ExitRequested (Cmd+Q / programmatic) and the last
            // window's CloseRequested go through this path; calling
            // shutdown() twice is harmless because the engine is taken
            // out of the lock on the first hit.
            //
            // kill -9 / OOM kills bypass this hook entirely — recovery
            // is handled by the WAL replay path on next startup
            // (covered by T-005's `append_then_load_replays_updates`
            // and the engine integration test).
            #[cfg(feature = "p2p")]
            {
                let should_shutdown = matches!(
                    _event,
                    RunEvent::ExitRequested { .. }
                        | RunEvent::WindowEvent {
                            event: WindowEvent::CloseRequested { .. },
                            ..
                        }
                );
                if should_shutdown {
                    if let Some(state) = _app_handle.try_state::<Arc<AppState>>() {
                        let engine = state.sync.write().take();
                        if let Some(engine) = engine {
                            // The event-loop callback is sync; block briefly
                            // to let shutdown await the checkpoint. The
                            // worst case is a few hundred ms — acceptable
                            // for graceful close.
                            tauri::async_runtime::block_on(async {
                                if let Err(e) = engine.shutdown().await {
                                    tracing::warn!("p2p shutdown on close: {e}");
                                }
                            });
                        }
                    }
                }
            }
        });
}
