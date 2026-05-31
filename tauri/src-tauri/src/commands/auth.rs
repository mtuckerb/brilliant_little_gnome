use super::AppStateArg;
use crate::error::{AppError, Result};
use crate::models::AuthStatus;
use tauri::AppHandle;
#[cfg(desktop)]
use std::time::Duration;
#[cfg(desktop)]
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

#[tauri::command]
pub async fn auth_status(state: AppStateArg<'_>) -> Result<AuthStatus> {
    let c = &state.client;
    Ok(AuthStatus {
        authenticated: c.is_configured(),
        degraded: c.is_degraded(),
        host: c.host_clone(),
        user_id: c.user_id_clone(),
        uid: c.uid_clone(),
    })
}

#[tauri::command]
pub async fn setup_cookies(
    state: AppStateArg<'_>,
    host: String,
    cookie_string: String,
    uid: Option<String>,
    user_id: Option<String>,
) -> Result<AuthStatus> {
    state
        .client
        .store_credentials(&state.pool, &host, &cookie_string, uid.as_deref(), user_id.as_deref())
        .await?;
    // Pre-share validation gates the device sync: the key is stored locally
    // above, but is only shared to peers if it validates as live. A failure
    // here surfaces an actionable, key-free error to the caller.
    #[cfg(feature = "p2p")]
    state.mirror_credentials_to_loro().await?;
    Ok(AuthStatus {
        authenticated: state.client.is_configured(),
        degraded: state.client.is_degraded(),
        host: state.client.host_clone(),
        user_id: state.client.user_id_clone(),
        uid: state.client.uid_clone(),
    })
}

#[tauri::command]
pub async fn clear_auth(state: AppStateArg<'_>) -> Result<()> {
    state.client.clear_credentials(&state.pool).await
}

#[derive(serde::Serialize)]
pub struct AuthExport {
    pub host: String,
    pub cookie: String,
}

/// Export the current session so the user can paste it into a fresh
/// install (faster than the full browser-login flow, simpler than P2P
/// pairing for one-off transfers). Errors when the device isn't
/// authenticated — there's nothing to export.
#[tauri::command]
pub async fn export_auth(state: AppStateArg<'_>) -> Result<AuthExport> {
    let host = state
        .client
        .host_clone()
        .ok_or_else(|| AppError::Other("No Brightspace host configured".into()))?;
    let cookie = state
        .client
        .cookie_clone()
        .ok_or_else(|| AppError::Other("Not signed in — nothing to export".into()))?;
    // Manual paste-export is also a share path: validate the key against the
    // known-good auth endpoint before handing it out, and fail closed.
    use crate::client::SessionValidation;
    match state.client.validate_session(&host, &cookie).await {
        SessionValidation::Valid => {}
        SessionValidation::Invalid => {
            return Err(AppError::BadRequest(
                crate::client::SHARE_BLOCKED_INVALID.into(),
            ))
        }
        SessionValidation::Inconclusive => {
            return Err(AppError::Other(
                crate::client::SHARE_BLOCKED_INCONCLUSIVE.into(),
            ))
        }
    }
    Ok(AuthExport { host, cookie })
}

/// Open a child webview window pointing at the Brightspace login URL, then poll
/// the runtime cookie store for the auth pair (`d2lSessionVal` +
/// `d2lSecureSessionVal`). When both arrive, persist them via
/// `store_credentials`, emit `auth-captured`, and close the window.
///
/// Desktop-only: mobile Tauri runs a single webview and doesn't expose
/// `set_focus` / `title` / `close` on `WebviewWindow`. The mobile sign-in
/// flow needs to happen in-app or via the system browser, which is a
/// separate piece of work — for now the mobile stub returns an error so
/// the JS layer can route around it.
#[cfg(desktop)]
#[tauri::command]
pub async fn open_login_window(app: AppHandle, host: String) -> Result<()> {
    let host = host.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/').to_string();
    if host.is_empty() {
        return Err(AppError::BadRequest("host required".into()));
    }
    let label = "brightspace-login".to_string();

    // Reuse window if already open.
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.set_focus();
    } else {
        let url_str = format!("https://{}/d2l/login", host);
        let url = url::Url::parse(&url_str).map_err(|e| AppError::BadRequest(e.to_string()))?;
        WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url))
            .title("Brightspace login")
            .inner_size(960.0, 720.0)
            .build()
            .map_err(|e| AppError::Other(format!("open login window: {}", e)))?;
    }

    // Spawn a polling task so the command can return immediately and the UI
    // stays responsive. Time out after 10 minutes.
    let app_for_task = app.clone();
    let host_for_task = host.clone();
    tauri::async_runtime::spawn(async move {
        let deadline = std::time::Instant::now() + Duration::from_secs(600);
        let target = match url::Url::parse(&format!("https://{}/", host_for_task)) {
            Ok(u) => u,
            Err(_) => return,
        };
        loop {
            if std::time::Instant::now() > deadline {
                let _ = app_for_task.emit("auth-capture-timeout", &host_for_task);
                break;
            }
            let Some(win) = app_for_task.get_webview_window(&label) else { break };
            match win.cookies_for_url(target.clone()) {
                Ok(cookies) => {
                    let mut sess = None::<String>;
                    let mut sec = None::<String>;
                    let mut all: Vec<String> = Vec::new();
                    for c in &cookies {
                        all.push(format!("{}={}", c.name(), c.value()));
                        match c.name() {
                            "d2lSessionVal" => sess = Some(c.value().to_string()),
                            "d2lSecureSessionVal" => sec = Some(c.value().to_string()),
                            _ => {}
                        }
                    }
                    if sess.is_some() && sec.is_some() {
                        let cookie_str = all.join("; ");
                        let st = app_for_task.state::<std::sync::Arc<crate::state::AppState>>();
                        let _ = st.client.store_credentials(&st.pool, &host_for_task, &cookie_str, None, None).await;
                        // Validate-before-share also gates this capture path,
                        // not just the primary command path. If the freshly
                        // captured key fails validation it is not shared; emit
                        // an actionable, key-free event so the UI can react.
                        #[cfg(feature = "p2p")]
                        if let Err(e) = st.mirror_credentials_to_loro().await {
                            let _ = app_for_task.emit("auth-share-blocked", e.to_string());
                        }
                        let _ = app_for_task.emit("auth-captured", &host_for_task);
                        let _ = win.close();
                        break;
                    }
                }
                Err(e) => {
                    tracing::debug!("cookies_for_url err: {}", e);
                }
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    });

    Ok(())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn open_login_window(_app: AppHandle, _host: String) -> Result<()> {
    Err(AppError::Other(
        "open_login_window is desktop-only — the mobile build needs an in-app sign-in flow".into(),
    ))
}
