//! Device-local content cache.
//!
//! A course can be "made available offline" — every cacheable topic (files,
//! LTI Tools, ContentService media) is fetched once and stored on the local
//! filesystem, with a metadata row in `content_cache`. Both consumers read
//! cache-first: the in-app viewer (`preview_topic_file`) and the course export
//! (`build_course_archive`).
//!
//! Quizzes and the other pure-quicklink types (discussion / dropbox / survey)
//! are never cached — a quiz isn't downloadable content, and the user asked
//! for them to be left untouched. Bare external links (Spotify, YouTube, …)
//! are bookmarks, not ours to cache.
//!
//! The bytes live under `<app_data_dir>/content-cache/<course>/<topic>` and are
//! intentionally NOT mirrored to Loro/P2P — cached media is large and
//! device-local, so it must never replicate across paired devices.

use super::AppStateArg;
use crate::error::{AppError, Result};
use serde::Serialize;
use std::{fs, path::PathBuf, time::Duration};
use tauri::{AppHandle, Manager};

/// Per-item fetch ceiling, mirroring the export walk so one slow/huge topic
/// can't stall a whole course cache run.
const CACHE_FETCH_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Default, Serialize)]
pub struct CacheSummary {
    pub cached: usize,
    pub skipped: usize,
    pub failed: usize,
    pub bytes: i64,
}

#[derive(Debug, Default, Serialize)]
pub struct CacheStatus {
    pub count: i64,
    pub bytes: i64,
    pub last_cached_at: Option<String>,
}

/// Cached bytes + the metadata a viewer needs to route/render them.
pub(crate) struct CachedContent {
    pub bytes: Vec<u8>,
    pub mime: Option<String>,
    pub filename: String,
}

/// Master switch. Defaults to on; a missing column (older DB) also reads as on.
pub(crate) async fn cache_enabled(pool: &sqlx::SqlitePool) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT cache_content FROM user_preferences LIMIT 1")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|v| v != 0)
        .unwrap_or(true)
}

fn cache_root(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(format!("could not resolve app data dir: {e}")))?
        .join("content-cache");
    Ok(dir)
}

/// Classify a content item into a cache kind, or `None` to skip it. Driven off
/// the already-synced `content_items` row (item_type + url) rather than the live
/// TOC, so no extra network call is needed to decide.
pub(crate) fn classify(item_type: Option<&str>, url: Option<&str>) -> Option<&'static str> {
    let t = item_type.unwrap_or_default().to_ascii_lowercase();
    let u = url.unwrap_or_default();
    match t.as_str() {
        "file" => Some("file"),
        "contentservice" => Some("media"),
        "link" => match quicklink_type(u).as_deref() {
            Some("lti") => Some("tool"),
            // quiz explicitly left untouched; the other quicklinks aren't
            // downloadable content.
            _ => None,
        },
        // A typeless topic with a Brightspace-relative path behaves like a file.
        "" if !u.is_empty() && !u.starts_with("http") => Some("file"),
        _ => None,
    }
}

/// Extract the `type=` value from a D2L quicklink URL (`…/quickLink.d2l?ou=…&type=quiz&…`).
fn quicklink_type(url: &str) -> Option<String> {
    let q = url.split('?').nth(1)?;
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some("type") {
            return it.next().map(|v| v.to_ascii_lowercase());
        }
    }
    None
}

/// Pull the destination out of an LTI auto-submit launch form. Brightspace
/// serves a tiny HTML page whose `<form action="…">` points at the vendor
/// resource; the OAuth params it carries expire within minutes, so the form
/// itself is worthless to store — the action URL is the durable part.
pub(crate) fn extract_lti_action(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let form = lower.find("<form")?;
    let action_at = lower[form..].find("action=")? + form + "action=".len();
    let rest = &html[action_at..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = rest[1..].find(quote)? + 1;
    let raw = &rest[1..end];
    if !raw.starts_with("http") {
        return None;
    }
    Some(raw.replace("&amp;", "&"))
}

/// The vendor URL an LTI Tool launches into, if we resolved one.
pub(crate) async fn get_resolved_url(
    pool: &sqlx::SqlitePool,
    course_id: &str,
    topic_id: &str,
) -> Option<String> {
    if !cache_enabled(pool).await {
        return None;
    }
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT resolved_url FROM content_cache WHERE course_id = ? AND topic_id = ?",
    )
    .bind(course_id)
    .bind(topic_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()
}

/// Look up a cached topic. Returns `None` if caching is off, nothing is cached,
/// or the backing file has gone missing (in which case the stale row is dropped).
///
/// Tools are deliberately excluded: their cached bytes are an expired launch
/// stub, and letting the export treat that as content produced empty files in
/// place of a working shortcut. Callers want `get_resolved_url` for those.
pub(crate) async fn get_cached(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    course_id: &str,
    topic_id: &str,
) -> Result<Option<CachedContent>> {
    if !cache_enabled(pool).await {
        return Ok(None);
    }
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT rel_path, filename, mime FROM content_cache
         WHERE course_id = ? AND topic_id = ? AND item_kind <> 'tool'",
    )
    .bind(course_id)
    .bind(topic_id)
    .fetch_optional(pool)
    .await?;
    let Some((rel_path, filename, mime)) = row else {
        return Ok(None);
    };
    let full = cache_root(app)?.join(&rel_path);
    match fs::read(&full) {
        Ok(bytes) => Ok(Some(CachedContent { bytes, mime, filename })),
        Err(_) => {
            // File vanished (manual delete / cache cleared out-of-band). Drop the
            // row so callers fall through to a live fetch.
            let _ = sqlx::query("DELETE FROM content_cache WHERE course_id = ? AND topic_id = ?")
                .bind(course_id)
                .bind(topic_id)
                .execute(pool)
                .await;
            Ok(None)
        }
    }
}

async fn put_cached(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    course_id: &str,
    topic_id: &str,
    filename: &str,
    mime: Option<&str>,
    kind: &str,
    bytes: &[u8],
    resolved_url: Option<&str>,
) -> Result<()> {
    // On-disk name is just <topic_id> (topic ids are filesystem-safe); the real
    // filename lives in the DB and is what the viewer routes on. Tools store no
    // bytes at all — only the resolved vendor URL is worth keeping.
    let rel_path = format!("{}/{}", course_id, topic_id);
    let stored_len = if kind == "tool" { 0 } else { bytes.len() as i64 };
    if kind != "tool" {
        let full = cache_root(app)?.join(&rel_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full, bytes)?;
    }
    sqlx::query(
        "INSERT INTO content_cache (course_id, topic_id, rel_path, filename, mime, byte_len, item_kind, resolved_url, fetched_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(course_id, topic_id) DO UPDATE SET
            rel_path = excluded.rel_path,
            filename = excluded.filename,
            mime = excluded.mime,
            byte_len = excluded.byte_len,
            item_kind = excluded.item_kind,
            resolved_url = excluded.resolved_url,
            fetched_at = CURRENT_TIMESTAMP",
    )
    .bind(course_id)
    .bind(topic_id)
    .bind(&rel_path)
    .bind(filename)
    .bind(mime)
    .bind(stored_len)
    .bind(kind)
    .bind(resolved_url)
    .execute(pool)
    .await?;
    Ok(())
}

/// Refresh LTI Tool destinations for a course, as part of the regular content
/// sync. A Tool's *content* can't be cached (see `extract_lti_action`), but
/// where its launch points is durable and worth keeping current — otherwise a
/// destination only appears after someone manually caches the course, and newly
/// added Tools would sit unresolved indefinitely.
///
/// Incremental by design: only Tools with no destination yet are fetched, so
/// this costs one pass and then nothing. Entirely best-effort — a Tool that
/// won't resolve leaves the export falling back to the D2L quicklink, which is
/// what it did before any of this existed.
pub(crate) async fn resolve_lti_destinations(state: &crate::state::AppState, course_id: &str) -> usize {
    if !cache_enabled(&state.pool).await {
        return 0;
    }
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT i.brightspace_id, i.url FROM content_items i
         WHERE i.module_id IN (SELECT brightspace_id FROM content_modules WHERE course_id = ?)
           AND i.url LIKE '%type=lti%'
           AND NOT EXISTS (
               SELECT 1 FROM content_cache c
               WHERE c.course_id = ? AND c.topic_id = i.brightspace_id
                 AND c.resolved_url IS NOT NULL
           )",
    )
    .bind(course_id)
    .bind(course_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut resolved = 0usize;
    for (topic_id, url) in rows {
        let Some(url) = url else { continue };
        let fetch = state.client.fetch_bytes(&url);
        let Ok(Ok((bytes, mime, _))) = tokio::time::timeout(CACHE_FETCH_TIMEOUT, fetch).await else {
            continue;
        };
        let Some(dest) = extract_lti_action(&String::from_utf8_lossy(&bytes)) else {
            continue;
        };
        let filename = format!("{}.html", topic_id);
        if put_cached(
            &state.app,
            &state.pool,
            course_id,
            &topic_id,
            &filename,
            mime.as_deref(),
            "tool",
            &bytes,
            Some(&dest),
        )
        .await
        .is_ok()
        {
            resolved += 1;
        }
    }
    resolved
}

/// Make a course available offline: cache every cacheable topic. Quizzes and
/// other non-content quicklinks are skipped. Re-running refreshes existing
/// entries.
#[tauri::command]
pub async fn cache_course_content(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<CacheSummary> {
    if !cache_enabled(&state.pool).await {
        return Err(AppError::Other(
            "Content caching is turned off in Settings.".to_string(),
        ));
    }
    state.client.preflight_auth().await?;

    let items: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT brightspace_id, item_type, url FROM content_items
         WHERE module_id IN (SELECT brightspace_id FROM content_modules WHERE course_id = ?)",
    )
    .bind(&course_id)
    .fetch_all(&state.pool)
    .await?;

    let mut sum = CacheSummary::default();
    for (topic_id, item_type, url) in items {
        let Some(kind) = classify(item_type.as_deref(), url.as_deref()) else {
            sum.skipped += 1;
            continue;
        };

        // 'file'/'media' come through the topic file endpoint; 'tool' fetches the
        // LTI quicklink itself (its launch page is HTML).
        let (path, forced_name): (String, Option<String>) = match kind {
            "tool" => match url.as_deref() {
                Some(u) => (u.to_string(), Some(format!("{}.html", topic_id))),
                None => {
                    sum.skipped += 1;
                    continue;
                }
            },
            _ => (
                format!(
                    "/d2l/api/le/{}/{}/content/topics/{}/file",
                    crate::client::API_VERSION,
                    course_id,
                    topic_id
                ),
                None,
            ),
        };

        let fetch = state.client.fetch_bytes(&path);
        match tokio::time::timeout(CACHE_FETCH_TIMEOUT, fetch).await {
            Ok(Ok((bytes, mime, header_name))) => {
                let filename = forced_name
                    .or(header_name)
                    .unwrap_or_else(|| format!("topic_{}", topic_id));
                // For a Tool, the payload is a throwaway launch form — keep only
                // where it points. No destination means nothing worth recording.
                let resolved = if kind == "tool" {
                    match extract_lti_action(&String::from_utf8_lossy(&bytes)) {
                        Some(u) => Some(u),
                        None => {
                            sum.failed += 1;
                            continue;
                        }
                    }
                } else {
                    None
                };
                if put_cached(
                    &app,
                    &state.pool,
                    &course_id,
                    &topic_id,
                    &filename,
                    mime.as_deref(),
                    kind,
                    &bytes,
                    resolved.as_deref(),
                )
                .await
                .is_ok()
                {
                    sum.cached += 1;
                    if kind != "tool" {
                        sum.bytes += bytes.len() as i64;
                    }
                } else {
                    sum.failed += 1;
                }
            }
            _ => sum.failed += 1,
        }
    }
    Ok(sum)
}

#[tauri::command]
pub async fn course_cache_status(state: AppStateArg<'_>, course_id: String) -> Result<CacheStatus> {
    let row: (i64, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(byte_len), 0), MAX(fetched_at)
         FROM content_cache WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(CacheStatus {
        count: row.0,
        bytes: row.1.unwrap_or(0),
        last_cached_at: row.2,
    })
}

#[tauri::command]
pub async fn clear_course_cache(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<()> {
    let dir = cache_root(&app)?.join(&course_id);
    let _ = fs::remove_dir_all(&dir); // best-effort; rows are the source of truth
    sqlx::query("DELETE FROM content_cache WHERE course_id = ?")
        .bind(&course_id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{classify, quicklink_type};

    #[test]
    fn files_and_media_cache_tools_too() {
        assert_eq!(classify(Some("File"), Some("/content/enforced/x.pptx")), Some("file"));
        assert_eq!(classify(Some("ContentService"), Some("d2l:brightspace:content:...")), Some("media"));
        assert_eq!(
            classify(Some("Link"), Some("/d2l/common/dialogs/quickLink/quickLink.d2l?ou=1&type=lti&rcode=x")),
            Some("tool")
        );
    }

    #[test]
    fn quizzes_and_other_quicklinks_are_skipped() {
        for ty in ["quiz", "discuss", "dropbox", "survey"] {
            let u = format!("/d2l/common/dialogs/quickLink/quickLink.d2l?ou=1&type={ty}&rcode=x");
            assert_eq!(classify(Some("Link"), Some(&u)), None, "type={ty} should skip");
        }
    }

    #[test]
    fn bare_external_links_are_skipped() {
        assert_eq!(classify(Some("Link"), Some("https://open.spotify.com/playlist/x")), None);
        assert_eq!(classify(Some("Link"), Some("https://youtu.be/x")), None);
    }

    #[test]
    fn extracts_lti_destination() {
        // Shape Brightspace actually serves (verified against a live OUP launch).
        let html = r#"<html><body><div id="ltiLaunchFormSubmitArea">
<form method="post" id="LtiRequestForm" action="https://iws.oupsupport.com/lti/1/arc/starr-waterman6e-timeline" enctype="application/x-www-form-urlencoded">
<input type="hidden" name="oauth_timestamp" value="1784500543"></form></body></html>"#;
        assert_eq!(
            super::extract_lti_action(html).as_deref(),
            Some("https://iws.oupsupport.com/lti/1/arc/starr-waterman6e-timeline")
        );
    }

    #[test]
    fn lti_extraction_unescapes_and_rejects_non_http() {
        let esc = r#"<form action="https://v.com/a?x=1&amp;y=2">"#;
        assert_eq!(super::extract_lti_action(esc).as_deref(), Some("https://v.com/a?x=1&y=2"));
        // A relative or missing action isn't a usable destination.
        assert_eq!(super::extract_lti_action(r#"<form action="/d2l/local">"#), None);
        assert_eq!(super::extract_lti_action("<html>no form</html>"), None);
    }

    #[test]
    fn quicklink_type_parses() {
        assert_eq!(
            quicklink_type("/d2l/common/dialogs/quickLink/quickLink.d2l?ou=1&type=lti&rcode=x").as_deref(),
            Some("lti")
        );
        assert_eq!(quicklink_type("https://youtu.be/x"), None);
    }
}
