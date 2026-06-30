// Brightspace HTTP client. Ports the behavior of `lib/brilliant/client.rb`:
// raw cookie auth, JSON GETs against /d2l/api/{lp,le}/<version>/..., paginated
// fetches via PagingInfo bookmarks, and a shared cache backed by `api_caches`.

use crate::error::{AppError, Result};
use futures::StreamExt;
use parking_lot::RwLock;
use reqwest::{header, Client, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

mod validation;
pub use validation::{SessionValidation, SHARE_BLOCKED_INCONCLUSIVE, SHARE_BLOCKED_INVALID};

pub const API_VERSION: &str = "1.40";
const FRESH_CACHE_SECONDS: i64 = 600;
const PAGE_SAFETY_LIMIT: usize = 2000;

pub struct BrightspaceClient {
    pub host: RwLock<Option<String>>,
    pub cookie: RwLock<Option<String>>,
    pub uid: RwLock<Option<String>>,
    pub user_id: RwLock<Option<String>>,
    pub degraded: RwLock<bool>,
    pub http: Client,
    app: AppHandle,
}

impl BrightspaceClient {
    pub async fn from_db(pool: &SqlitePool, app: AppHandle) -> Result<Self> {
        let row: Option<(Option<String>, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT brightspace_host, brightspace_cookie, brightspace_uid, brightspace_user_id FROM user_preferences LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;
        let (host, cookie, uid, user_id) = row.unwrap_or((None, None, None, None));

        let http = Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            host: RwLock::new(host),
            cookie: RwLock::new(cookie),
            uid: RwLock::new(uid),
            user_id: RwLock::new(user_id),
            degraded: RwLock::new(false),
            http,
            app,
        })
    }

    pub fn is_configured(&self) -> bool {
        self.host.read().is_some()
            && self
                .cookie
                .read()
                .as_ref()
                .map(|c| c.contains('=') && c.len() > 20)
                .unwrap_or(false)
    }

    pub fn is_degraded(&self) -> bool { *self.degraded.read() }

    /// Validate a candidate Brightspace session key against the known-good
    /// auth endpoint before it is shared/synced to another device. Reuses
    /// the same `/users/whoami` liveness signal as `maybe_mark_auth_failure`,
    /// but never logs the key and fails closed (a misconfigured or
    /// unreachable endpoint yields `Inconclusive`, not `Valid`).
    pub async fn validate_session(&self, host: &str, cookie: &str) -> SessionValidation {
        let probe_url = match validation::validation_probe_url(host) {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!("auth validation: cannot resolve probe url: {e}");
                return SessionValidation::Inconclusive;
            }
        };
        validation::probe_session(&self.http, &probe_url, cookie).await
    }

    /// Live preflight check for expensive bulk operations (course/module
    /// archives). Probes `/users/whoami` directly via `validate_session` —
    /// bypassing the JSON cache — so a dead session fails fast with a single
    /// "session expired" toast instead of 403-ing every per-item fetch and
    /// emitting a zip full of skips. Only blocks when the session is
    /// definitively `Invalid` (or no credentials exist); an `Inconclusive`
    /// probe (network blip, gateway hiccup) is allowed through so a transient
    /// failure can't strand a legitimate export. On a definitive failure it
    /// reuses `commit_auth_failure`, so the same `authentication_failure`
    /// app-event the rest of the client emits drives the toast.
    pub async fn preflight_auth(&self) -> Result<()> {
        let host = self.host.read().clone();
        let cookie = self.cookie.read().clone();
        let (Some(host), Some(cookie)) = (host, cookie) else {
            // No credentials at all — definitively unauthenticated.
            self.commit_auth_failure(StatusCode::UNAUTHORIZED);
            return Err(AppError::Unauthenticated);
        };

        match self.validate_session(&host, &cookie).await {
            SessionValidation::Valid => {
                *self.degraded.write() = false;
                Ok(())
            }
            SessionValidation::Invalid => {
                tracing::warn!("preflight auth: session expired; aborting bulk download");
                self.commit_auth_failure(StatusCode::FORBIDDEN);
                Err(AppError::Unauthenticated)
            }
            SessionValidation::Inconclusive => {
                // Couldn't confirm the session is dead — don't block the export.
                tracing::warn!("preflight auth: probe inconclusive; proceeding with download");
                Ok(())
            }
        }
    }

    pub async fn store_credentials(
        &self,
        pool: &SqlitePool,
        host: &str,
        cookie: &str,
        uid: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<()> {
        let normalized_host = host.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or(host).trim().to_string();
        let normalized_cookie = normalize_cookie_string(cookie);

        sqlx::query(
            "UPDATE user_preferences SET brightspace_host = ?, brightspace_cookie = ?, brightspace_uid = ?, brightspace_user_id = ?, last_login_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT id FROM user_preferences LIMIT 1)",
        )
        .bind(&normalized_host)
        .bind(&normalized_cookie)
        .bind(uid)
        .bind(user_id)
        .execute(pool)
        .await?;
        *self.host.write() = Some(normalized_host);
        *self.cookie.write() = Some(normalized_cookie);
        *self.uid.write() = uid.map(|s| s.to_string());
        *self.user_id.write() = user_id.map(|s| s.to_string());
        *self.degraded.write() = false;
        Ok(())
    }

    pub async fn clear_credentials(&self, pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            "UPDATE user_preferences SET brightspace_cookie = NULL, brightspace_uid = NULL, brightspace_user_id = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT id FROM user_preferences LIMIT 1)",
        )
        .execute(pool)
        .await?;
        *self.cookie.write() = None;
        *self.uid.write() = None;
        *self.user_id.write() = None;
        Ok(())
    }

    pub fn host_clone(&self) -> Option<String> { self.host.read().clone() }
    pub fn cookie_clone(&self) -> Option<String> { self.cookie.read().clone() }
    pub fn uid_clone(&self) -> Option<String> { self.uid.read().clone() }
    pub fn user_id_clone(&self) -> Option<String> { self.user_id.read().clone() }

    /// Absorb a rotated Brightspace session cookie from a response's
    /// `Set-Cookie` headers. Brightspace periodically reissues
    /// `d2lSessionVal` / `d2lSecureSessionVal` to extend the session; we don't
    /// run a reqwest cookie jar (cookies are attached as a static header), so
    /// without this those rotations are discarded and the stored cookie ages
    /// out — which on a sync-enabled device also strands paired peers on a
    /// stale shared cookie. This replaces only the rotated values in the
    /// in-memory cookie string (preserving order and any other cookies) and
    /// returns true if anything changed, so the sync layer can persist +
    /// re-share the fresh value. Never clears or corrupts the cookie: empty or
    /// unparsable Set-Cookie entries are ignored.
    pub fn absorb_rotated_cookie(&self, headers: &header::HeaderMap) -> bool {
        let set_cookies: Vec<&str> = headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|hv| hv.to_str().ok())
            .collect();
        let mut guard = self.cookie.write();
        let Some(current) = guard.clone() else { return false };
        match merge_rotated_cookie(&current, &set_cookies) {
            Some(updated) => {
                *guard = Some(updated);
                true
            }
            None => false,
        }
    }

    /// Emit the same `auth-captured` event the desktop login flow does.
    /// Used by the P2P bootstrap path to wake up the React layer (which
    /// re-fetches auth-status and starts a sync) after the joiner adopts
    /// a paired device's credentials.
    pub fn emit_auth_captured(&self, host: &str) -> std::result::Result<(), tauri::Error> {
        self.app.emit("auth-captured", host)
    }

    /// Emit a non-secret, actionable auth-share failure message for UI paths
    /// that adopt or publish credentials outside the primary login command.
    pub fn emit_auth_share_blocked(&self, message: &str) -> std::result::Result<(), tauri::Error> {
        self.app.emit("auth-share-blocked", message)
    }

    /// Probe `/users/whoami` to confirm the session is genuinely dead
    /// before flipping the app into "degraded" state. A surprising number
    /// of 401/403s come from individual endpoints (resource-scoped perms,
    /// transient gateway hiccups) rather than session expiry. Without
    /// this probe we kept showing "session expired" toasts on a healthy
    /// account, which trained the user to ignore them.
    async fn maybe_mark_auth_failure(&self, origin_path: &str, status: StatusCode) {
        // Already-known-degraded: don't bother re-probing — saves API
        // chatter while the user is mid-reauth.
        if self.is_degraded() {
            return;
        }
        let host = self.host.read().clone();
        let cookie = self.cookie.read().clone();
        let (Some(host), Some(cookie)) = (host, cookie) else {
            // No creds to probe with — the failure is real.
            self.commit_auth_failure(status);
            return;
        };
        let probe_url = format!("https://{}/d2l/api/lp/{}/users/whoami", host, API_VERSION);
        match self
            .http
            .get(&probe_url)
            .header(header::COOKIE, cookie)
            .send()
            .await
        {
            Ok(resp) => {
                let probe_status = resp.status();
                if probe_status.is_success() {
                    tracing::info!(
                        "auth probe passed (whoami {}) despite {} on {}; not flipping degraded",
                        probe_status,
                        status,
                        origin_path,
                    );
                    return;
                }
                if matches!(probe_status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                    tracing::warn!(
                        "auth probe confirmed expiry (whoami {}); marking degraded",
                        probe_status,
                    );
                    self.commit_auth_failure(status);
                } else {
                    tracing::warn!(
                        "auth probe inconclusive (whoami {}); leaving degraded=false",
                        probe_status,
                    );
                }
            }
            Err(e) => {
                tracing::warn!("auth probe network error: {} — not flipping degraded", e);
            }
        }
    }

    fn commit_auth_failure(&self, status: StatusCode) {
        *self.degraded.write() = true;
        let host = self.host.read().clone().unwrap_or_default();
        if let Err(e) = self.app.emit(
            "app-event",
            json!({
                "kind": "authentication_failure",
                "code": status.as_u16(),
                "host": host,
            }),
        ) {
            tracing::warn!("event emit failed for authentication_failure: {}", e);
        }
    }

    fn is_resource_scoped_auth_failure(path: &str, status: StatusCode) -> bool {
        status == StatusCode::FORBIDDEN && is_discussion_resource_path(path)
    }

    /// GET a JSON path with cache-aware semantics. Returns `Value` so callers can
    /// decide whether the payload is an array, an Objects-wrapped page, etc.
    ///
    /// Pagination cursors (`?bookmark=...`) are intentionally **not** persisted to
    /// `api_caches`: bookmarks are opaque, change every fetch, and would never be
    /// re-hit — caching them just bloats the table.
    pub async fn do_get(&self, pool: &SqlitePool, path: &str, force_refresh: bool) -> Result<Value> {
        let path = normalize_path(path);
        let is_bookmark_page = path.contains("bookmark=");
        let cached = if is_bookmark_page { None } else { read_cache(pool, &path).await? };

        if !force_refresh {
            if let Some((value, age)) = &cached {
                if *age < FRESH_CACHE_SECONDS {
                    return Ok(value.clone());
                }
            }
        }

        if !self.is_configured() {
            return cached
                .map(|(v, _)| v)
                .ok_or(AppError::Unauthenticated);
        }

        match self.fetch(&path).await {
            Ok(value) => {
                if !is_bookmark_page {
                    write_cache(pool, &path, &value).await?;
                }
                Ok(value)
            }
            Err(e) => {
                if let Some((v, _)) = cached {
                    // Cache fall-back is an expected, recoverable path — demoted
                    // from warn so a clean sync prints zero WARN lines under
                    // default RUST_LOG. Bump to RUST_LOG=debug if you need to
                    // see which paths fell back.
                    tracing::debug!("fetch {} failed, using cache: {}", path, e);
                    Ok(v)
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn fetch(&self, path: &str) -> Result<Value> {
        let host = self
            .host
            .read()
            .clone()
            .ok_or_else(|| AppError::Other("no Brightspace host configured".into()))?;
        let cookie = self.cookie.read().clone().ok_or(AppError::Unauthenticated)?;
        let url = format!("https://{}{}", host, path);

        tracing::debug!("GET {}", url);
        let resp = self
            .http
            .get(&url)
            .header(header::ACCEPT, "application/json")
            .header(header::COOKIE, cookie)
            .send()
            .await?;

        // Pick up any rotated session cookie before the body consumes `resp`.
        self.absorb_rotated_cookie(resp.headers());

        let status = resp.status();
        if status.is_success() {
            *self.degraded.write() = false;
            let value: Value = resp.json().await?;
            // Brightspace error envelopes occasionally return 200 with Errors array.
            if value.is_object() && (value.get("Errors").is_some() || value.get("ErrorMessage").is_some()) {
                let body = value.to_string();
                return Err(AppError::BrightspaceApi { status: 200, body });
            }
            Ok(value)
        } else if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            // 401s and most 403s are treated as global auth failures. Discussion
            // API 403s are often course/forum/topic-scoped permissions problems;
            // do not force global re-auth for those unless an independent auth
            // validation call fails elsewhere.
            if Self::is_resource_scoped_auth_failure(path, status) {
                // Discussion forum/topic 403s are expected for areas the user
                // doesn't have permission to read (instructor-only forums,
                // restricted topics). Demoted from warn to keep clean syncs
                // free of recurring noise.
                tracing::debug!(
                    "resource-scoped Brightspace auth failure for {}: {}",
                    path,
                    status
                );
            } else {
                self.maybe_mark_auth_failure(path, status).await;
            }
            let body = resp.text().await.unwrap_or_default();
            Err(AppError::BrightspaceApi { status: status.as_u16(), body })
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(AppError::BrightspaceApi { status: status.as_u16(), body })
        }
    }

    /// Paginated fetch following PagingInfo.HasMoreItems + Bookmark.
    pub async fn get_all_pages(&self, pool: &SqlitePool, path: &str, force_refresh: bool) -> Result<Vec<Value>> {
        let mut items: Vec<Value> = Vec::new();
        let mut current = path.to_string();
        let base = path.to_string();
        let mut force = force_refresh;

        loop {
            let data = self.do_get(pool, &current, force).await?;
            let page_items = ensure_array(&data);
            let has_more = data
                .get("PagingInfo")
                .and_then(|p| p.get("HasMoreItems"))
                .and_then(|b| b.as_bool())
                .unwrap_or(false);
            let bookmark = data
                .get("PagingInfo")
                .and_then(|p| p.get("Bookmark"))
                .and_then(|b| b.as_str())
                .map(|s| s.to_string());

            items.extend(page_items);

            if items.len() > PAGE_SAFETY_LIMIT {
                break;
            }

            match (has_more, bookmark) {
                (true, Some(bm)) => {
                    let sep = if base.contains('?') { '&' } else { '?' };
                    current = format!("{}{}bookmark={}", base, sep, urlencoding(&bm));
                    force = true; // any subsequent page must be fresh
                }
                _ => break,
            }
        }
        Ok(items)
    }

    /// PUT a JSON body to a Brightspace path. Used by mark-as-read style
    /// state updates (`/discussions/topics/.../MyReadStatus`). Returns
    /// nothing — these endpoints are write-only fire-and-forget.
    pub async fn put_json(&self, path: &str, body: serde_json::Value) -> Result<()> {
        let host = self.host.read().clone()
            .ok_or_else(|| AppError::Other("no Brightspace host configured".into()))?;
        let cookie = self.cookie.read().clone().ok_or(AppError::Unauthenticated)?;
        let url = format!("https://{}{}", host, path);
        let resp = self.http.put(&url)
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json")
            .body(body.to_string())
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                self.maybe_mark_auth_failure(path, status).await;
            }
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::BrightspaceApi { status: status.as_u16(), body });
        }
        *self.degraded.write() = false;
        Ok(())
    }

    /// Fetch a raw HTML page (not JSON, not cached). Used by the PSY-220
    /// scraper which has to parse the rendered grades view.
    pub async fn fetch_html(&self, path: &str) -> Result<String> {
        let host = self.host.read().clone()
            .ok_or_else(|| AppError::Other("no Brightspace host configured".into()))?;
        let cookie = self.cookie.read().clone().ok_or(AppError::Unauthenticated)?;
        let url = format!("https://{}{}", host, path);
        let resp = self.http.get(&url)
            .header(header::ACCEPT, "text/html,application/xhtml+xml")
            .header(header::COOKIE, cookie)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                self.maybe_mark_auth_failure(path, status).await;
            }
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::BrightspaceApi { status: status.as_u16(), body });
        }
        *self.degraded.write() = false;
        Ok(resp.text().await?)
    }

    /// Fetch arbitrary binary content (PDFs, attachments, etc.) with the user's
    /// auth cookie. Bypasses the JSON cache because attachments aren't JSON and
    /// can be large. Returns `(bytes, content_type, suggested_filename)`. The
    /// filename is parsed from the `Content-Disposition` header when present.
    pub async fn fetch_bytes(&self, url_or_path: &str) -> Result<(Vec<u8>, Option<String>, Option<String>)> {
        self.fetch_bytes_with_limit(url_or_path, None).await
    }

    /// Same as `fetch_bytes`, but enforces a hard byte cap before/while reading
    /// the response body. `Content-Length` is rejected up front when present, and
    /// chunked/missing-length responses are aborted once they exceed the cap.
    pub async fn fetch_bytes_with_limit(&self, url_or_path: &str, max_bytes: Option<usize>) -> Result<(Vec<u8>, Option<String>, Option<String>)> {
        let host = self.host.read().clone()
            .ok_or_else(|| AppError::Other("no Brightspace host configured".into()))?;
        let cookie = self.cookie.read().clone().ok_or(AppError::Unauthenticated)?;

        // Brightspace attachment URLs may be absolute (https://…) or root-relative.
        let url = if url_or_path.starts_with("http://") || url_or_path.starts_with("https://") {
            url_or_path.to_string()
        } else {
            format!("https://{}{}", host, url_or_path)
        };

        let resp = self.http.get(&url)
            .header(header::COOKIE, cookie)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                self.maybe_mark_auth_failure(url_or_path, status).await;
            }
            return Err(AppError::BrightspaceApi { status: status.as_u16(), body: format!("GET {} -> {}", url, status) });
        }
        *self.degraded.write() = false;
        if let (Some(max), Some(length)) = (max_bytes, resp.content_length()) {
            if length > max as u64 {
                return Err(AppError::Other("File too large to preview — open externally".to_string()));
            }
        }
        let content_type = resp.headers().get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let filename = resp.headers().get(header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_filename_from_content_disposition);
        let mut bytes = Vec::with_capacity(max_bytes.unwrap_or_default().min(resp.content_length().unwrap_or(0) as usize));
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if let Some(max) = max_bytes {
                if bytes.len().saturating_add(chunk.len()) > max {
                    return Err(AppError::Other("File too large to preview — open externally".to_string()));
                }
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((bytes, content_type, filename))
    }

    pub async fn archive_cache(&self, pool: &SqlitePool, path: &str) -> Result<()> {
        sqlx::query("UPDATE api_caches SET is_archived = 1, updated_at = CURRENT_TIMESTAMP WHERE path = ?")
            .bind(normalize_path(path))
            .execute(pool)
            .await?;
        Ok(())
    }

    // ---- Endpoint helpers ------------------------------------------------

    pub async fn get_who_am_i(&self, pool: &SqlitePool) -> Result<Value> {
        self.do_get(pool, &format!("/d2l/api/lp/{}/users/whoami", API_VERSION), false).await
    }

    pub async fn get_enrollments(&self, pool: &SqlitePool, force_refresh: bool) -> Result<Vec<Value>> {
        let data = self.do_get(pool, &format!("/d2l/api/lp/{}/enrollments/myenrollments/", API_VERSION), force_refresh).await?;
        let items = ensure_array(&data);
        // Filter to course offerings only.
        let filtered: Vec<Value> = items.into_iter().filter(|i| {
            let code = i.pointer("/OrgUnit/Type/Code").and_then(|v| v.as_str());
            let name = i.pointer("/OrgUnit/Type/Name").and_then(|v| v.as_str());
            code == Some("Course Offering") || name == Some("Course Offering")
        }).collect();
        Ok(filtered)
    }

    pub async fn get_toc(&self, pool: &SqlitePool, course_id: &str, force_refresh: bool) -> Result<Value> {
        let path = format!("/d2l/api/le/{}/{}/content/toc", API_VERSION, course_id);
        let data = self.do_get(pool, &path, force_refresh).await?;
        // Normalize to { Modules: [...] }.
        if data.is_array() {
            return Ok(serde_json::json!({ "Modules": data }));
        }
        if data.get("Modules").is_some() {
            return Ok(data);
        }
        Ok(serde_json::json!({ "Modules": [] }))
    }

    pub async fn get_assignments(&self, pool: &SqlitePool, course_id: &str, force_refresh: bool) -> Result<Vec<Value>> {
        let path = format!("/d2l/api/le/{}/{}/dropbox/folders/", API_VERSION, course_id);
        self.get_all_pages(pool, &path, force_refresh).await
    }

    pub async fn get_assignment_folder(&self, pool: &SqlitePool, course_id: &str, folder_id: &str, force_refresh: bool) -> Result<Value> {
        let path = format!("/d2l/api/le/{}/{}/dropbox/folders/{}", API_VERSION, course_id, folder_id);
        self.do_get(pool, &path, force_refresh).await
    }

    pub async fn get_assignment_feedback(&self, pool: &SqlitePool, course_id: &str, folder_id: &str, force_refresh: bool) -> Result<Value> {
        let path = format!("/d2l/api/le/{}/{}/dropbox/folders/{}/feedback/myFeedback", API_VERSION, course_id, folder_id);
        self.do_get(pool, &path, force_refresh).await
    }

    pub async fn get_assignment_submissions(&self, pool: &SqlitePool, course_id: &str, folder_id: &str, force_refresh: bool) -> Result<Value> {
        let path = format!("/d2l/api/le/{}/{}/dropbox/folders/{}/submissions/", API_VERSION, course_id, folder_id);
        self.do_get(pool, &path, force_refresh).await
    }

    pub async fn get_assignment_rubrics(&self, pool: &SqlitePool, course_id: &str, force_refresh: bool) -> Result<Value> {
        let path = format!("/d2l/api/le/{}/{}/feedback/rubrics/", API_VERSION, course_id);
        self.do_get(pool, &path, force_refresh).await
    }

    pub async fn get_quizzes(&self, pool: &SqlitePool, course_id: &str, force_refresh: bool) -> Result<Vec<Value>> {
        let path = format!("/d2l/api/le/{}/{}/quizzes/", API_VERSION, course_id);
        self.get_all_pages(pool, &path, force_refresh).await
    }

    pub async fn get_grades(&self, pool: &SqlitePool, course_id: &str, force_refresh: bool) -> Result<Vec<Value>> {
        let path = format!("/d2l/api/le/{}/{}/grades/values/myGradeValues/", API_VERSION, course_id);
        self.get_all_pages(pool, &path, force_refresh).await
    }

    pub async fn get_grade_definitions(&self, pool: &SqlitePool, course_id: &str, force_refresh: bool) -> Result<Vec<Value>> {
        let path = format!("/d2l/api/le/{}/{}/grades/", API_VERSION, course_id);
        self.get_all_pages(pool, &path, force_refresh).await
    }

    pub async fn get_discussion_forums(&self, pool: &SqlitePool, course_id: &str, force_refresh: bool) -> Result<Vec<Value>> {
        let path = format!("/d2l/api/le/{}/{}/discussions/forums/", API_VERSION, course_id);
        let data = self.do_get(pool, &path, force_refresh).await?;
        Ok(ensure_array(&data))
    }

    pub async fn get_discussion_topics(&self, pool: &SqlitePool, course_id: &str, forum_id: &str, force_refresh: bool) -> Result<Vec<Value>> {
        let path = format!("/d2l/api/le/{}/{}/discussions/forums/{}/topics/", API_VERSION, course_id, forum_id);
        let data = self.do_get(pool, &path, force_refresh).await?;
        Ok(ensure_array(&data))
    }

    pub async fn get_discussion_posts(&self, pool: &SqlitePool, course_id: &str, forum_id: &str, topic_id: &str, force_refresh: bool) -> Result<Vec<Value>> {
        let path = format!(
            "/d2l/api/le/{}/{}/discussions/forums/{}/topics/{}/posts/",
            API_VERSION, course_id, forum_id, topic_id,
        );
        self.get_all_pages(pool, &path, force_refresh).await
    }

    pub async fn get_global_alerts(&self, pool: &SqlitePool, since: Option<&str>) -> Result<Vec<Value>> {
        let mut path = format!("/d2l/api/lp/{}/alerts/", API_VERSION);
        if let Some(s) = since {
            path.push_str("?since=");
            path.push_str(&urlencoding(s));
        }
        let data = self.do_get(pool, &path, true).await?;
        Ok(ensure_array(&data))
    }

    pub async fn get_unified_feed(&self, pool: &SqlitePool, since: Option<&str>, force_refresh: bool) -> Result<Vec<Value>> {
        let mut path = format!("/d2l/api/lp/{}/feed/", API_VERSION);
        if let Some(s) = since {
            path.push_str("?since=");
            path.push_str(&urlencoding(s));
        }
        let data = self.do_get(pool, &path, force_refresh).await?;
        Ok(ensure_array(&data))
    }

    /// A course's announcements ("News" items). This is the authoritative,
    /// complete per-course list — unlike the global `/lp/feed/`, which only
    /// surfaces a subset of course announcements, so it's the source we use to
    /// avoid silently dropping things like a "room change" notice.
    pub async fn get_course_news(&self, pool: &SqlitePool, course_id: &str, force_refresh: bool) -> Result<Vec<Value>> {
        let path = format!("/d2l/api/le/{}/{}/news/", API_VERSION, course_id);
        let data = self.do_get(pool, &path, force_refresh).await?;
        Ok(ensure_array(&data))
    }

    pub async fn get_overview(&self, pool: &SqlitePool, course_id: &str, force_refresh: bool) -> Result<Value> {
        let path = format!("/d2l/api/le/{}/{}/overview", API_VERSION, course_id);
        self.do_get(pool, &path, force_refresh).await
    }

    /// Typed helper for callers that know the shape they want.
    #[allow(dead_code)]
    pub async fn do_get_as<T: DeserializeOwned>(&self, pool: &SqlitePool, path: &str, force_refresh: bool) -> Result<T> {
        let v = self.do_get(pool, path, force_refresh).await?;
        Ok(serde_json::from_value(v)?)
    }
}

pub type SharedClient = Arc<BrightspaceClient>;

// ---- helpers ------------------------------------------------------------

pub fn ensure_array(data: &Value) -> Vec<Value> {
    match data {
        Value::Array(a) => a.clone(),
        Value::Object(o) => {
            if let Some(Value::Array(a)) = o.get("Objects") { return a.clone(); }
            if let Some(Value::Array(a)) = o.get("Items") { return a.clone(); }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn is_discussion_resource_path(path: &str) -> bool {
    let path_only = path.split('?').next().unwrap_or(path);
    let Some(idx) = path_only.find("/discussions/") else { return false; };
    let rest = &path_only[idx + "/discussions/".len()..];
    rest == "topics/"
        || rest == "topics"
        || rest.starts_with("topics/")
        || rest == "forums/"
        || rest == "forums"
        || rest.starts_with("forums/")
}

fn normalize_path(p: &str) -> String {
    // Collapse double slashes inside the path (Ruby `gsub('//', '/')`).
    let mut out = String::with_capacity(p.len());
    let mut prev = '\0';
    for c in p.chars() {
        if c == '/' && prev == '/' {
            continue;
        }
        out.push(c);
        prev = c;
    }
    out
}

fn normalize_cookie_string(c: &str) -> String {
    let mut s = c.to_string();
    if let Some(rest) = s.strip_prefix("Cookie:") {
        s = rest.trim().to_string();
    } else if let Some(rest) = s.strip_prefix("cookie:") {
        s = rest.trim().to_string();
    }
    s.replace(['\r', '\n'], "; ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Parse RFC 6266 `Content-Disposition` filename, supporting both `filename=`
/// and the UTF-8 `filename*=UTF-8''…` form. Returns the decoded basename only.
fn parse_filename_from_content_disposition(cd: &str) -> Option<String> {
    // Prefer filename* (UTF-8) when present.
    for part in cd.split(';') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("filename*=") {
            // filename*=UTF-8''<percent-encoded>
            if let Some(idx) = rest.find("''") {
                let raw = &rest[idx + 2..];
                return Some(percent_decode(raw));
            }
            return Some(strip_quotes(rest).to_string());
        }
    }
    for part in cd.split(';') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("filename=") {
            return Some(strip_quotes(rest).to_string());
        }
    }
    None
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

async fn read_cache(pool: &SqlitePool, path: &str) -> Result<Option<(Value, i64)>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT data, updated_at FROM api_caches WHERE path = ? AND is_archived = 0",
    )
    .bind(path)
    .fetch_optional(pool)
    .await?;
    let Some((data, updated_at)) = row else { return Ok(None); };
    let value: Value = serde_json::from_str(&data)?;
    let age = age_seconds(&updated_at);
    Ok(Some((value, age)))
}

async fn write_cache(pool: &SqlitePool, path: &str, data: &Value) -> Result<()> {
    let json = serde_json::to_string(data)?;
    sqlx::query(
        "INSERT INTO api_caches (path, data, is_archived, updated_at) VALUES (?, ?, 0, CURRENT_TIMESTAMP)
         ON CONFLICT(path) DO UPDATE SET data = excluded.data, is_archived = 0, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(path)
    .bind(json)
    .execute(pool)
    .await?;
    Ok(())
}

fn age_seconds(updated_at: &str) -> i64 {
    use chrono::{DateTime, NaiveDateTime, Utc};
    // Try RFC3339 first ("...Z"), then SQLite default ("YYYY-MM-DD HH:MM:SS").
    let dt: Option<DateTime<Utc>> = DateTime::parse_from_rfc3339(updated_at)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            NaiveDateTime::parse_from_str(updated_at, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|n| n.and_utc())
        });
    match dt {
        Some(t) => (Utc::now() - t).num_seconds(),
        None => i64::MAX,
    }
}

/// Merge any rotated `d2lSessionVal` / `d2lSecureSessionVal` from a set of
/// `Set-Cookie` header strings into `current`, preserving order and other
/// cookies. Returns `Some(new_cookie)` only when a tracked value actually
/// changed, `None` when nothing rotated. Never drops or blanks an existing
/// cookie — empty or unparsable Set-Cookie entries are ignored, so a bad
/// response can't corrupt the stored session.
fn merge_rotated_cookie(current: &str, set_cookies: &[&str]) -> Option<String> {
    const ROTATING: [&str; 2] = ["d2lSessionVal", "d2lSecureSessionVal"];
    let mut updates: Vec<(&str, &str)> = Vec::new();
    for sc in set_cookies {
        let first = sc.split(';').next().unwrap_or("");
        let Some((name, value)) = first.split_once('=') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if ROTATING.contains(&name) && !value.is_empty() {
            updates.push((name, value));
        }
    }
    if updates.is_empty() {
        return None;
    }

    let mut pairs: Vec<(String, String)> = current
        .split(';')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }
            let (n, v) = p.split_once('=')?;
            Some((n.trim().to_string(), v.trim().to_string()))
        })
        .collect();

    let mut changed = false;
    for (n, v) in updates {
        match pairs.iter_mut().find(|(pn, _)| pn == n) {
            Some(slot) if slot.1 != v => {
                slot.1 = v.to_string();
                changed = true;
            }
            Some(_) => {}
            None => {
                pairs.push((n.to_string(), v.to_string()));
                changed = true;
            }
        }
    }
    if !changed {
        return None;
    }
    Some(
        pairs
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

#[cfg(test)]
mod tests {
    use super::{is_discussion_resource_path, merge_rotated_cookie, BrightspaceClient};
    use reqwest::StatusCode;

    #[test]
    fn rotated_secure_cookie_is_merged_in_place() {
        let current = "d2lSessionVal=AAA; d2lSecureSessionVal=BBB; other=keep";
        let got = merge_rotated_cookie(current, &["d2lSecureSessionVal=CCC; Path=/; HttpOnly"]);
        assert_eq!(
            got.as_deref(),
            Some("d2lSessionVal=AAA; d2lSecureSessionVal=CCC; other=keep")
        );
    }

    #[test]
    fn unchanged_value_yields_none() {
        let current = "d2lSessionVal=AAA; d2lSecureSessionVal=BBB";
        assert_eq!(merge_rotated_cookie(current, &["d2lSecureSessionVal=BBB; Path=/"]), None);
    }

    #[test]
    fn untracked_or_empty_set_cookies_are_ignored() {
        let current = "d2lSessionVal=AAA; d2lSecureSessionVal=BBB";
        // An unrelated cookie and an empty value must not touch the session.
        assert_eq!(merge_rotated_cookie(current, &["analytics=1; Path=/", "d2lSessionVal=; Path=/"]), None);
    }

    #[test]
    fn both_session_cookies_rotate_at_once() {
        let current = "d2lSessionVal=AAA; d2lSecureSessionVal=BBB";
        let got = merge_rotated_cookie(
            current,
            &["d2lSessionVal=NEW1; Path=/", "d2lSecureSessionVal=NEW2; Secure"],
        );
        assert_eq!(got.as_deref(), Some("d2lSessionVal=NEW1; d2lSecureSessionVal=NEW2"));
    }

    #[test]
    fn discussion_forum_and_topic_403s_are_resource_scoped() {
        let paths = [
            "/d2l/api/le/1.40/447090/discussions/forums/",
            "/d2l/api/le/1.40/447090/discussions/forums/375900/topics/",
            "/d2l/api/le/1.40/447090/discussions/forums/375900/topics/123/posts/",
            "/d2l/api/le/1.40/447090/discussions/topics/",
        ];
        for path in paths {
            assert!(is_discussion_resource_path(path), "{path}");
            assert!(BrightspaceClient::is_resource_scoped_auth_failure(path, StatusCode::FORBIDDEN), "{path}");
        }
    }

    #[test]
    fn non_discussion_or_401_auth_failures_remain_global() {
        assert!(!BrightspaceClient::is_resource_scoped_auth_failure(
            "/d2l/api/lp/1.40/users/whoami",
            StatusCode::FORBIDDEN,
        ));
        assert!(!BrightspaceClient::is_resource_scoped_auth_failure(
            "/d2l/api/le/1.40/447090/discussions/forums/375900/topics/",
            StatusCode::UNAUTHORIZED,
        ));
    }
}
