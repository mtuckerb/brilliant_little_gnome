//! Single-file and bulk-archive download commands.
//!
//! The original Sinatra app spun up a background `DownloadJob` model that
//! streamed a ZIP back over HTTP. In the Tauri port we collapse that to a
//! synchronous command that returns base64'd bytes — the frontend converts to
//! a Blob and triggers a normal browser download via an anchor element. This
//! reuses the same blob-download flow already proven in `SyllabusPanel`.
//!
//! Bulk archives are written using the in-tree `zip_writer` (see this file).
//! Only the STORED method is supported (no compression) so we don't depend on
//! the full `zip` crate, which isn't in the offline cargo cache.

use super::AppStateArg;
use crate::error::{AppError, Result};
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashSet, fs, path::PathBuf, time::Duration};
use tauri::{AppHandle, Emitter};

const DOWNLOAD_EVENT: &str = "download://saved";

/// Upper bound on any single file fetch during a course archive build. The HTTP
/// client already caps a request at 30s, but a slow-streaming body or an
/// expensive post-fetch step (e.g. html→markdown on a pathological page) can
/// still stall one item and, because the archive walk is sequential, drag the
/// whole export out until the caller gives up (PSY-100 hung ~4min this way).
/// Bounding each item and recording a timeout as a per-file failure keeps one
/// bad topic from stalling an otherwise good archive.
const ARCHIVE_FETCH_TIMEOUT: Duration = Duration::from_secs(45);

/// `client.fetch_bytes` with a hard per-item ceiling. Used only by the archive
/// collectors; single-file user downloads keep the client's own timeout.
async fn fetch_bytes_bounded(
    state: &crate::state::AppState,
    url_or_path: &str,
) -> Result<(Vec<u8>, Option<String>, Option<String>)> {
    match tokio::time::timeout(ARCHIVE_FETCH_TIMEOUT, state.client.fetch_bytes(url_or_path)).await {
        Ok(result) => result,
        Err(_) => Err(AppError::Other(format!(
            "timed out after {}s",
            ARCHIVE_FETCH_TIMEOUT.as_secs()
        ))),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadBytes {
    /// Base64 payload retained as a browser fallback; desktop downloads are
    /// written directly to disk so Tauri/WebView blob-download quirks do not
    /// silently swallow the file.
    pub bytes_base64: Option<String>,
    pub mime: Option<String>,
    pub filename: String,
    pub saved_path: Option<String>,
}

fn emit_saved(app: &AppHandle, payload: &DownloadBytes) {
    // Best-effort: never let a missed listener break a real download.
    if let Err(e) = app.emit(DOWNLOAD_EVENT, payload) {
        tracing::warn!("emit {} failed: {}", DOWNLOAD_EVENT, e);
    }
}

#[tauri::command]
pub async fn download_topic_file(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
    topic_id: String,
) -> Result<DownloadBytes> {
    let path = format!(
        "/d2l/api/le/{}/{}/content/topics/{}/file",
        crate::client::API_VERSION,
        course_id,
        topic_id
    );
    let (bytes, mime, name) = state.client.fetch_bytes(&path).await?;
    let filename = name.unwrap_or_else(|| format!("topic_{}.bin", topic_id));
    let saved_path = save_download_file(&state.app, &filename, &bytes)?;
    let payload = DownloadBytes {
        bytes_base64: None,
        mime,
        filename,
        saved_path: Some(saved_path.display().to_string()),
    };
    emit_saved(&app, &payload);
    Ok(payload)
}

#[tauri::command]
pub async fn download_module_archive(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
    module_id: String,
) -> Result<DownloadBytes> {
    // Preflight the session before the expensive module walk (see
    // download_course_archive for rationale).
    state.client.preflight_auth().await?;

    // Pull the module title for both the archive name and the top-level folder
    // inside the zip. Falls back to the raw id if the row has been pruned.
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT title FROM content_modules WHERE course_id = ? AND brightspace_id = ?",
    )
    .bind(&course_id)
    .bind(&module_id)
    .fetch_optional(&state.pool)
    .await?;
    let module_title = row.map(|(t,)| t).unwrap_or_else(|| format!("Module-{}", module_id));

    // Walk the module tree (this module + recursive children). Brightspace
    // returns the structure in a single GET, so we don't paginate.
    let toc = state.client.get_toc(&state.pool, &course_id, false).await?;
    let module_node = find_module(&toc, &module_id).ok_or_else(|| {
        AppError::Other(format!("module {} not found in TOC", module_id))
    })?;

    let mut zip = zip_writer::Builder::new();
    let mut seen = HashSet::new();
    let mut failures = Vec::new();
    collect_module(&mut zip, state.inner().as_ref(), &course_id, &module_node, "", &mut seen, &mut failures).await?;

    // If the session expired mid-walk every live fetch 403'd and the zip is
    // useless — surface the auth failure (toast already emitted by the probe)
    // instead of saving an archive full of nothing.
    if state.client.is_degraded() {
        return Err(AppError::Unauthenticated);
    }
    if zip.entry_count() == 0 {
        return Err(AppError::Other("No downloadable files found in this module.".to_string()));
    }
    add_download_warnings(&mut zip, &failures);

    let bytes = zip.finish();
    let filename = format!("Brilliant-{}-{}.zip", course_id, sanitize(&module_title));
    let saved_path = save_download_file(&state.app, &filename, &bytes)?;
    let payload = DownloadBytes {
        bytes_base64: None,
        mime: Some("application/zip".to_string()),
        filename,
        saved_path: Some(saved_path.display().to_string()),
    };
    emit_saved(&app, &payload);
    Ok(payload)
}

#[tauri::command]
pub async fn download_course_archive(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<DownloadBytes> {
    let (bytes, _failures) = build_course_archive(state.inner().as_ref(), &course_id).await?;
    let filename = format!("Brilliant-{}.zip", course_id);
    let saved_path = save_download_file(&state.app, &filename, &bytes)?;
    tracing::info!("download_course_archive saved {}", saved_path.display());
    let payload = DownloadBytes {
        bytes_base64: None,
        mime: Some("application/zip".to_string()),
        filename,
        saved_path: Some(saved_path.display().to_string()),
    };
    emit_saved(&app, &payload);
    Ok(payload)
}

/// Build a full course archive (zip bytes + failure notes) WITHOUT writing to
/// disk. Shared by the Tauri command above (which saves) and the REST
/// `/courses/:id/export` endpoint (which streams it back for gbrain to unpack
/// into the vault). HTML content pages, the course overview, and announcements
/// are converted to Markdown so the archive reads natively in an Obsidian
/// vault; binary files (PDF/docx/images/…) are included as-is.
pub(crate) async fn build_course_archive(
    state: &crate::state::AppState,
    course_id: &str,
) -> Result<(Vec<u8>, Vec<String>)> {
    tracing::info!("build_course_archive start course={}", course_id);
    // Preflight: confirm the session is live before the expensive walk so a
    // dead session fails fast instead of producing a zip full of 403 skips.
    state.client.preflight_auth().await?;
    let toc = state.client.get_toc(&state.pool, course_id, false).await?;
    let modules = toc.get("Modules").and_then(|m| m.as_array()).cloned().unwrap_or_default();

    let mut zip = zip_writer::Builder::new();
    let mut seen = HashSet::new();
    let mut failures = Vec::new();
    for m in &modules {
        if let Err(e) = collect_module(&mut zip, state, course_id, m, "Table_of_Contents/", &mut seen, &mut failures).await {
            tracing::warn!("module skipped during course archive: {}", e);
            failures.push(format!("module {} skipped: {}", module_title(m), e));
        }
    }

    collect_assignment_files(&mut zip, state, course_id, &mut seen, &mut failures).await;
    collect_overview_attachments(&mut zip, state, course_id, &mut seen, &mut failures).await;
    collect_overview_markdown(&mut zip, state, course_id, &mut seen).await;
    collect_announcements_markdown(&mut zip, state, course_id, &mut seen).await;

    // Best-effort: fetch the course overview attachment (syllabus) if one exists.
    let overview_path = format!("/d2l/api/le/{}/{}/overview", crate::client::API_VERSION, course_id);
    if let Ok(ov) = state.client.do_get(&state.pool, &overview_path, false).await {
        if let Some(att_url) = ov
            .pointer("/Attachment/Url")
            .and_then(|v| v.as_str())
            .or_else(|| ov.pointer("/Attachment/Href").and_then(|v| v.as_str()))
        {
            match fetch_bytes_bounded(state, att_url).await {
                Ok((bytes, _mime, name)) => {
                    let fname = name
                        .or_else(|| {
                            ov.pointer("/Attachment/Name")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        })
                        .unwrap_or_else(|| "syllabus".to_string());
                    let path = unique_path(&mut seen, &format!("Syllabus_Overview/{}", sanitize(&fname)));
                    zip.add_file(&path, &bytes);
                }
                Err(e) => failures.push(format!("overview attachment skipped: {}", e)),
            }
        }
    }

    tracing::info!(
        "build_course_archive course={} entries={} failures={}",
        course_id,
        zip.entry_count(),
        failures.len()
    );
    if state.client.is_degraded() {
        return Err(AppError::Unauthenticated);
    }
    if zip.entry_count() == 0 {
        return Err(AppError::Other(format!(
            "No downloadable files found in this course. {} failures encountered. \
             First failure: {}",
            failures.len(),
            failures.first().map(String::as_str).unwrap_or("(none)")
        )));
    }
    add_download_warnings(&mut zip, &failures);
    Ok((zip.finish(), failures))
}

/// Convert the course overview HTML to `_Overview.md`.
async fn collect_overview_markdown(
    zip: &mut zip_writer::Builder,
    state: &crate::state::AppState,
    course_id: &str,
    seen: &mut HashSet<String>,
) {
    let path = format!("/d2l/api/le/{}/{}/overview", crate::client::API_VERSION, course_id);
    if let Ok(ov) = state.client.do_get(&state.pool, &path, false).await {
        let html = ov
            .pointer("/Description/Html")
            .and_then(|v| v.as_str())
            .or_else(|| ov.pointer("/Description/Text").and_then(|v| v.as_str()));
        if let Some(h) = html {
            if !h.trim().is_empty() {
                let md = crate::commands::htmlmd::html_to_markdown(h);
                let entry = unique_path(seen, "_Overview.md");
                zip.add_file(&entry, format!("# Course Overview\n\n{}\n", md).as_bytes());
            }
        }
    }
}

/// Roll the course's announcements (already synced into `notifications`) into a
/// single `_Announcements.md`, newest first.
async fn collect_announcements_markdown(
    zip: &mut zip_writer::Builder,
    state: &crate::state::AppState,
    course_id: &str,
    seen: &mut HashSet<String>,
) {
    let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT title, body, date FROM notifications \
         WHERE notification_type = 'News' AND course_id = ? ORDER BY date DESC NULLS LAST",
    )
    .bind(course_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    if rows.is_empty() {
        return;
    }
    let mut md = String::from("# Announcements\n\n");
    for (title, body, date) in rows {
        md.push_str(&format!("## {}\n\n", title));
        if let Some(d) = date {
            md.push_str(&format!("*{}*\n\n", &d[..d.len().min(10)]));
        }
        if let Some(b) = body {
            md.push_str(crate::commands::htmlmd::html_to_markdown(&b).trim());
            md.push_str("\n\n");
        }
        md.push_str("---\n\n");
    }
    let entry = unique_path(seen, "_Announcements.md");
    zip.add_file(&entry, md.as_bytes());
}

/// Download the course's overview attachment (syllabus PDF) straight to disk
/// via the same save flow as every other download. Replaces the SyllabusPanel
/// blob-URL anchor-click path that WKWebView silently swallowed.
#[tauri::command]
pub async fn download_course_syllabus(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<DownloadBytes> {
    let overview = state.client.get_overview(&state.pool, &course_id, false).await?;
    let has_attachment = overview
        .get("HasAttachment")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || overview.pointer("/Attachment/Url").is_some();
    if !has_attachment {
        return Err(AppError::Other("This course's overview has no attachment.".to_string()));
    }
    // Brightspace omits the URL from /overview JSON when an attachment exists —
    // the bytes live at /overview/attachment. Fall back to the constructed path
    // when no explicit URL is present in the cached JSON.
    let att_url = overview
        .pointer("/Attachment/Url")
        .and_then(|v| v.as_str())
        .or_else(|| overview.pointer("/Attachment/Href").and_then(|v| v.as_str()))
        .map(String::from)
        .unwrap_or_else(|| crate::commands::overview::overview_attachment_path(&course_id));
    let suggested = overview
        .pointer("/Attachment/Name")
        .and_then(|v| v.as_str())
        .unwrap_or("Syllabus")
        .to_string();

    let (bytes, mime, response_name) = state.client.fetch_bytes(&att_url).await?;
    let filename = response_name.unwrap_or(suggested);
    let saved_path = save_download_file(&state.app, &filename, &bytes)?;
    let payload = DownloadBytes {
        bytes_base64: None,
        mime,
        filename,
        saved_path: Some(saved_path.display().to_string()),
    };
    emit_saved(&app, &payload);
    Ok(payload)
}

/// Open a URL in the user's default browser. Used by the "open in
/// Brightspace" buttons. Not part of tauri-plugin-shell so the React code
/// doesn't need an allowlist capability — the Rust side calls the OS
/// directly. Only http(s) URLs are accepted to avoid being a generic
/// `Command::new` proxy.
#[tauri::command]
pub fn open_url(url: String) -> Result<()> {
    use std::process::Command;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(AppError::Other(format!("refusing to open non-http URL: {}", url)));
    }

    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(&url).spawn();

    #[cfg(target_os = "windows")]
    let result = Command::new("cmd").args(["/C", "start", "", &url]).spawn();

    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open").arg(&url).spawn();

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let result: std::io::Result<_> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "open_url not supported on this platform",
    ));

    result.map_err(|e| AppError::Other(format!("open_url: {}", e)))?;
    Ok(())
}

/// Reveal a previously-saved file in the OS file manager (Finder on macOS,
/// Explorer on Windows, the parent folder on Linux).
#[tauri::command]
pub fn reveal_in_folder(path: String) -> Result<()> {
    use std::process::Command;
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::Other(format!("File no longer exists: {}", path)));
    }

    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg("-R").arg(&p).spawn();

    #[cfg(target_os = "windows")]
    let result = Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();

    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open")
        .arg(p.parent().unwrap_or(&p))
        .spawn();

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let result: std::io::Result<_> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "reveal_in_folder not supported on this platform",
    ));

    result.map_err(|e| AppError::Other(format!("reveal_in_folder: {}", e)))?;
    Ok(())
}

fn find_module(toc: &Value, target_id: &str) -> Option<Value> {
    let modules = toc.get("Modules").and_then(|m| m.as_array())?;
    for m in modules {
        if module_id_str(m).as_deref() == Some(target_id) {
            return Some(m.clone());
        }
        if let Some(found) = find_module_recursive(m, target_id) {
            return Some(found);
        }
    }
    None
}

fn find_module_recursive(node: &Value, target: &str) -> Option<Value> {
    let subs = node.get("Modules").and_then(|m| m.as_array())?;
    for m in subs {
        if module_id_str(m).as_deref() == Some(target) {
            return Some(m.clone());
        }
        if let Some(found) = find_module_recursive(m, target) {
            return Some(found);
        }
    }
    None
}

fn module_id_str(m: &Value) -> Option<String> {
    m.get("Identifier")
        .or_else(|| m.get("ModuleId"))
        .or_else(|| m.get("Id"))
        .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))
}

fn module_title(m: &Value) -> String {
    m.get("Title")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| module_id_str(m).unwrap_or_else(|| "unknown".to_string()))
}

fn topic_id_str(t: &Value) -> Option<String> {
    t.get("Identifier")
        .or_else(|| t.get("TopicId"))
        .or_else(|| t.get("Id"))
        .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))
}

fn topic_download_path(course_id: &str, topic_id: &str) -> String {
    format!(
        "/d2l/api/le/{}/{}/content/topics/{}/file",
        crate::client::API_VERSION,
        course_id,
        topic_id
    )
}

#[allow(clippy::too_many_arguments)]
async fn collect_module(
    zip: &mut zip_writer::Builder,
    state: &crate::state::AppState,
    course_id: &str,
    node: &Value,
    parent_prefix: &str,
    seen: &mut HashSet<String>,
    failures: &mut Vec<String>,
) -> Result<()> {
    let title = node
        .get("Title")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Untitled".to_string());
    let folder = format!("{}{}/", parent_prefix, sanitize(&title));

    if let Some(topics) = node.get("Topics").and_then(|t| t.as_array()) {
        for t in topics {
            let topic_id = match topic_id_str(t) {
                Some(id) => id,
                None => continue,
            };
            let topic_title = t
                .get("Title")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| format!("topic_{}", topic_id));
            let url = t.get("Url").and_then(|v| v.as_str());
            if is_external_link_topic(t, url) {
                if let Some(url) = url {
                    let entry = unique_path(seen, &format!("{}{}.url", folder, sanitize(&topic_title)));
                    let host = state.client.host_clone();
                    zip.add_file(&entry, internet_shortcut(url, host.as_deref()).as_bytes());
                    continue;
                }
            }
            let path = topic_download_path(course_id, &topic_id);
            match fetch_bytes_bounded(state, &path).await {
                Ok((bytes, mime, name)) => {
                    // HTML content pages → Markdown so they're readable in the
                    // vault; everything else (PDF/docx/images/…) stays as-is.
                    let is_html = mime.as_deref().map_or(false, |m| m.contains("html"))
                        || name.as_deref().map_or(false, |n| {
                            let l = n.to_ascii_lowercase();
                            l.ends_with(".html") || l.ends_with(".htm")
                        });
                    if is_html {
                        let md = crate::commands::htmlmd::html_to_markdown(&String::from_utf8_lossy(&bytes));
                        let entry = unique_path(seen, &format!("{}{}.md", folder, sanitize(&topic_title)));
                        zip.add_file(&entry, format!("# {}\n\n{}\n", topic_title, md).as_bytes());
                    } else {
                        let fname = name.unwrap_or_else(|| filename_with_extension(&topic_title, None));
                        let entry = unique_path(seen, &format!("{}{}", folder, fname));
                        zip.add_file(&entry, &bytes);
                    }
                }
                Err(e) => {
                    tracing::debug!("topic {} skipped: {}", topic_id, e);
                    failures.push(format!("topic '{}' ({}) skipped: {}", topic_title, topic_id, e));
                }
            }
        }
    }

    if let Some(subs) = node.get("Modules").and_then(|m| m.as_array()) {
        for sub in subs {
            Box::pin(collect_module(zip, state, course_id, sub, &folder, seen, failures)).await?;
        }
    }

    Ok(())
}

async fn collect_assignment_files(
    zip: &mut zip_writer::Builder,
    state: &crate::state::AppState,
    course_id: &str,
    seen: &mut HashSet<String>,
    failures: &mut Vec<String>,
) {
    let Ok(assignments) = state.client.get_assignments(&state.pool, course_id, false).await else {
        return;
    };

    for assignment in assignments {
        let Some(assignment_id) = value_id(&assignment, &["Id", "FolderId"]) else { continue; };
        let assignment_name = assignment
            .get("Name")
            .or_else(|| assignment.get("Title"))
            .and_then(|v| v.as_str())
            .unwrap_or("Assignment");
        let folder = format!("Assignments/{}/", sanitize(assignment_name));

        match state.client.get_assignment_folder(&state.pool, course_id, &assignment_id, false).await {
            Ok(detail) => add_attachments(zip, state, seen, failures, &folder, &detail, &["Attachments", "AttachedResources"]).await,
            Err(e) => failures.push(format!("assignment '{}' detail skipped: {}", assignment_name, e)),
        }

        if let Ok(feedback) = state.client.get_assignment_feedback(&state.pool, course_id, &assignment_id, false).await {
            add_attachments(zip, state, seen, failures, &format!("{}Feedback/", folder), &feedback, &["Attachments", "AttachedResources"]).await;
        }
    }
}

async fn collect_overview_attachments(
    zip: &mut zip_writer::Builder,
    state: &crate::state::AppState,
    course_id: &str,
    seen: &mut HashSet<String>,
    failures: &mut Vec<String>,
) {
    let Ok(overview) = state.client.get_overview(&state.pool, course_id, false).await else {
        return;
    };
    add_attachments(zip, state, seen, failures, "Syllabus_Overview/", &overview, &["Attachments", "LinkAttachments"]).await;
}

async fn add_attachments(
    zip: &mut zip_writer::Builder,
    state: &crate::state::AppState,
    seen: &mut HashSet<String>,
    failures: &mut Vec<String>,
    folder: &str,
    owner: &Value,
    keys: &[&str],
) {
    for key in keys {
        if let Some(items) = owner.get(*key).and_then(|v| v.as_array()) {
            let shortcut_links = key.to_ascii_lowercase().contains("link");
            for item in items {
                add_attachment(zip, state, seen, failures, folder, item, shortcut_links).await;
            }
        }
    }
}

async fn add_attachment(
    zip: &mut zip_writer::Builder,
    state: &crate::state::AppState,
    seen: &mut HashSet<String>,
    failures: &mut Vec<String>,
    folder: &str,
    attachment: &Value,
    shortcut_link: bool,
) {
    let name = attachment_name(attachment);
    let url = attachment_url(attachment);
    let Some(url) = url else { return; };

    if shortcut_link {
        let entry = unique_path(seen, &format!("{}{}.url", folder, sanitize(&name)));
        let host = state.client.host_clone();
        zip.add_file(&entry, internet_shortcut(&url, host.as_deref()).as_bytes());
        return;
    }

    match fetch_bytes_bounded(state, &url).await {
        Ok((bytes, _mime, response_name)) => {
            let fname = response_name.unwrap_or_else(|| filename_with_extension(&name, None));
            let entry = unique_path(seen, &format!("{}{}", folder, sanitize(&fname)));
            zip.add_file(&entry, &bytes);
        }
        Err(e) => failures.push(format!("attachment '{}' skipped: {}", name, e)),
    }
}

fn is_external_link_topic(topic: &Value, url: Option<&str>) -> bool {
    let type_hint = topic
        .get("TypeIdentifier")
        .or_else(|| topic.get("Type"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    type_hint.contains("link")
        || type_hint == "3"
        || url.map(|u| u.starts_with("http://") || u.starts_with("https://")).unwrap_or(false)
}

fn attachment_name(attachment: &Value) -> String {
    attachment
        .get("FileName")
        .or_else(|| attachment.get("Name"))
        .or_else(|| attachment.get("Title"))
        .or_else(|| attachment.get("LinkName"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "attachment".to_string())
}

fn attachment_url(attachment: &Value) -> Option<String> {
    attachment
        .get("Href")
        .or_else(|| attachment.get("Link"))
        .or_else(|| attachment.get("Url"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn value_id(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k))
        .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))
}

fn filename_with_extension(name: &str, fallback_ext: Option<&str>) -> String {
    let safe = sanitize(name);
    if safe.contains('.') || fallback_ext.is_none() {
        safe
    } else {
        format!("{}{}", safe, fallback_ext.unwrap_or(""))
    }
}

/// Build a `.url` Internet Shortcut. Brightspace hands us two shapes of link:
/// absolute (`https://youtube.com/…`) and scheme-less D2L quicklinks
/// (`/d2l/common/dialogs/quickLink/…`). A shortcut whose `URL=` has no host is
/// invalid — the OS can't open it — which is why the relative ones landed in
/// the vault "corrupt". Resolve those against the Brightspace host so every
/// shortcut is a real absolute URL.
fn internet_shortcut(url: &str, host: Option<&str>) -> String {
    let abs = absolutize_link(url, host);
    format!("[InternetShortcut]\r\nURL={}\r\n", abs)
}

/// Resolve a Brightspace link to an absolute URL. Root-relative paths (`/d2l/…`)
/// get the host prepended; anything already absolute (or, as a last resort, a
/// relative link with no host available) is passed through unchanged.
fn absolutize_link(url: &str, host: Option<&str>) -> String {
    if url.starts_with('/') && !url.starts_with("//") {
        if let Some(h) = host {
            return format!("https://{}{}", h, url);
        }
    }
    url.to_string()
}

fn add_download_warnings(zip: &mut zip_writer::Builder, failures: &[String]) {
    if failures.is_empty() {
        return;
    }
    let mut body = String::from("Some files could not be included in this archive.\n\n");
    for failure in failures {
        body.push_str("- ");
        body.push_str(failure);
        body.push('\n');
    }
    zip.add_file("DOWNLOAD_WARNINGS.txt", body.as_bytes());
}

fn sanitize(name: &str) -> String {
    let cleaned = name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() { "untitled".to_string() } else { cleaned }
}

/// Resolve the platform-appropriate save destination and write `data` there.
///
/// On iOS we MUST write to the app's sandboxed Documents directory — that's
/// the only place visible to the Files app, and `dirs::download_dir()` returns
/// None inside the sandbox anyway. On macOS/Linux we keep the user's
/// Downloads folder so the file lands somewhere familiar.
pub(crate) fn save_download_file(
    app: &tauri::AppHandle,
    filename: &str,
    data: &[u8],
) -> Result<PathBuf> {
    #[cfg(target_os = "ios")]
    let dest = {
        use tauri::Manager;
        app.path()
            .document_dir()
            .map_err(|e| AppError::Other(format!("could not resolve iOS Documents dir: {e}")))?
    };

    #[cfg(not(target_os = "ios"))]
    let dest = {
        let _ = app; // keep arg used on non-iOS without an unused-arg warning
        dirs::download_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
            .ok_or_else(|| AppError::Other("Could not determine a Downloads folder".to_string()))?
    };

    fs::create_dir_all(&dest)?;
    let mut path = unique_filesystem_path(&dest, &sanitize(filename));
    fs::write(&path, data)?;

    // Canonicalization is best-effort because some platforms/sandboxes may not
    // allow resolving a user-visible location. Returning the original path is
    // still more useful than failing after a successful write.
    if let Ok(canonical) = path.canonicalize() {
        path = canonical;
    }
    Ok(path)
}

fn unique_filesystem_path(dir: &std::path::Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }

    let (stem, ext) = match filename.rfind('.') {
        Some(i) if i > 0 => (&filename[..i], &filename[i..]),
        _ => (filename, ""),
    };
    let mut n = 1;
    loop {
        let attempt = dir.join(format!("{}_{}{}", stem, n, ext));
        if !attempt.exists() {
            return attempt;
        }
        n += 1;
    }
}

fn unique_path(seen: &mut HashSet<String>, candidate: &str) -> String {
    if !seen.contains(candidate) {
        seen.insert(candidate.to_string());
        return candidate.to_string();
    }
    let (stem, ext) = match candidate.rfind('.') {
        Some(i) if i > 0 && !candidate[..i].ends_with('/') => (&candidate[..i], &candidate[i..]),
        _ => (candidate, ""),
    };
    let mut n = 1;
    loop {
        let attempt = format!("{}_{}{}", stem, n, ext);
        if !seen.contains(&attempt) {
            seen.insert(attempt.clone());
            return attempt;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{filename_with_extension, sanitize, topic_download_path, unique_filesystem_path, unique_path};
    use std::{collections::HashSet, fs};

    #[test]
    fn downloads_sanitize_replaces_filesystem_reserved_characters() {
        assert_eq!(
            sanitize(r#" unit/lesson\quiz:1*?"<>| .pdf "#),
            "unit_lesson_quiz_1_______.pdf"
        );
    }

    #[test]
    fn downloads_sanitize_falls_back_for_empty_names() {
        assert_eq!(sanitize("   \t\n"), "untitled");
    }

    #[test]
    fn downloads_filename_with_extension_appends_fallback_only_when_missing() {
        assert_eq!(filename_with_extension("Lecture Notes", Some(".pdf")), "Lecture Notes.pdf");
        assert_eq!(filename_with_extension("Lecture Notes.docx", Some(".pdf")), "Lecture Notes.docx");
        assert_eq!(filename_with_extension(" bad/name ", Some(".txt")), "bad_name.txt");
    }

    #[test]
    fn downloads_unique_path_deduplicates_zip_entries_before_extension() {
        let mut seen = HashSet::new();

        assert_eq!(unique_path(&mut seen, "Module/Slides.pdf"), "Module/Slides.pdf");
        assert_eq!(unique_path(&mut seen, "Module/Slides.pdf"), "Module/Slides_1.pdf");
        assert_eq!(unique_path(&mut seen, "Module/Slides.pdf"), "Module/Slides_2.pdf");
    }

    #[test]
    fn downloads_unique_path_handles_extensionless_entries() {
        let mut seen = HashSet::new();

        assert_eq!(unique_path(&mut seen, "Module/README"), "Module/README");
        assert_eq!(unique_path(&mut seen, "Module/README"), "Module/README_1");
    }

    #[test]
    fn downloads_unique_filesystem_path_skips_existing_collisions() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("report.pdf"), b"existing").expect("seed first collision");
        fs::write(dir.path().join("report_1.pdf"), b"existing").expect("seed second collision");

        assert_eq!(unique_filesystem_path(dir.path(), "report.pdf"), dir.path().join("report_2.pdf"));
    }

    #[test]
    fn downloads_bulk_topic_path_matches_single_file_auth_path() {
        // Regression: the bulk archive loop must fetch topic files through the
        // exact same authenticated LE endpoint the single-file
        // `download_topic_file` command uses. If these ever diverge, bulk
        // downloads can silently lose auth and 403 every item.
        assert_eq!(
            topic_download_path("476750", "12009482"),
            format!(
                "/d2l/api/le/{}/476750/content/topics/12009482/file",
                crate::client::API_VERSION
            )
        );
    }
}

// ----- Minimal ZIP writer (STORED only) ----------------------------------

mod zip_writer {
    //! Hand-rolled ZIP writer producing STORED (uncompressed) archives. We
    //! deliberately avoid pulling in the `zip` crate — it isn't in the offline
    //! cargo cache for this build environment, and the STORED format is
    //! sufficient for our use case (most LMS files are already PDFs / images /
    //! pptx, all of which compress poorly anyway).
    //!
    //! Format references:
    //! - Local file header: PK\x03\x04
    //! - Central directory header: PK\x01\x02
    //! - End of central directory: PK\x05\x06
    //! - https://en.wikipedia.org/wiki/ZIP_(file_format)#Structure

    pub struct Builder {
        body: Vec<u8>,
        entries: Vec<Entry>,
    }

    struct Entry {
        name: Vec<u8>,
        crc32: u32,
        size: u32,
        offset: u32,
        mod_time: u16,
        mod_date: u16,
    }

    impl Builder {
        pub fn new() -> Self {
            Self { body: Vec::new(), entries: Vec::new() }
        }

        pub fn entry_count(&self) -> usize {
            self.entries.len()
        }

        pub fn add_file(&mut self, path: &str, data: &[u8]) {
            let crc = crc32fast::hash(data);
            let name = path.as_bytes().to_vec();
            let offset = self.body.len() as u32;

            // Local file header
            self.body.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            self.body.extend_from_slice(&20u16.to_le_bytes()); // version needed
            self.body.extend_from_slice(&0u16.to_le_bytes()); // flags
            self.body.extend_from_slice(&0u16.to_le_bytes()); // method (stored)
            let (mod_time, mod_date) = dos_now();
            self.body.extend_from_slice(&mod_time.to_le_bytes());
            self.body.extend_from_slice(&mod_date.to_le_bytes());
            self.body.extend_from_slice(&crc.to_le_bytes());
            self.body.extend_from_slice(&(data.len() as u32).to_le_bytes()); // compressed size
            self.body.extend_from_slice(&(data.len() as u32).to_le_bytes()); // uncompressed size
            self.body.extend_from_slice(&(name.len() as u16).to_le_bytes());
            self.body.extend_from_slice(&0u16.to_le_bytes()); // extra length
            self.body.extend_from_slice(&name);
            self.body.extend_from_slice(data);

            self.entries.push(Entry {
                name,
                crc32: crc,
                size: data.len() as u32,
                offset,
                mod_time,
                mod_date,
            });
        }

        pub fn finish(mut self) -> Vec<u8> {
            let cd_offset = self.body.len() as u32;

            for e in &self.entries {
                self.body.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
                self.body.extend_from_slice(&20u16.to_le_bytes()); // version made by
                self.body.extend_from_slice(&20u16.to_le_bytes()); // version needed
                self.body.extend_from_slice(&0u16.to_le_bytes()); // flags
                self.body.extend_from_slice(&0u16.to_le_bytes()); // method
                self.body.extend_from_slice(&e.mod_time.to_le_bytes());
                self.body.extend_from_slice(&e.mod_date.to_le_bytes());
                self.body.extend_from_slice(&e.crc32.to_le_bytes());
                self.body.extend_from_slice(&e.size.to_le_bytes());
                self.body.extend_from_slice(&e.size.to_le_bytes());
                self.body.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
                self.body.extend_from_slice(&0u16.to_le_bytes()); // extra
                self.body.extend_from_slice(&0u16.to_le_bytes()); // comment
                self.body.extend_from_slice(&0u16.to_le_bytes()); // disk
                self.body.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
                self.body.extend_from_slice(&0u32.to_le_bytes()); // external attrs
                self.body.extend_from_slice(&e.offset.to_le_bytes());
                self.body.extend_from_slice(&e.name);
            }

            let cd_size = self.body.len() as u32 - cd_offset;

            // End-of-central-directory record
            self.body.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
            self.body.extend_from_slice(&0u16.to_le_bytes()); // disk number
            self.body.extend_from_slice(&0u16.to_le_bytes()); // disk with cd
            self.body.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
            self.body.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
            self.body.extend_from_slice(&cd_size.to_le_bytes());
            self.body.extend_from_slice(&cd_offset.to_le_bytes());
            self.body.extend_from_slice(&0u16.to_le_bytes()); // comment length

            self.body
        }
    }

    fn dos_now() -> (u16, u16) {
        // DOS time: hours << 11 | minutes << 5 | seconds/2
        // DOS date: (year-1980) << 9 | month << 5 | day
        use chrono::{Datelike, Local, Timelike};
        let now = Local::now();
        let time = ((now.hour() as u16) << 11)
            | ((now.minute() as u16) << 5)
            | ((now.second() / 2) as u16);
        let year = (now.year() - 1980).max(0) as u16;
        let date = (year << 9) | ((now.month() as u16) << 5) | (now.day() as u16);
        (time, date)
    }
}

#[cfg(test)]
mod url_shortcut_tests {
    use super::{absolutize_link, internet_shortcut};

    #[test]
    fn relative_quicklink_gets_host() {
        let out = absolutize_link("/d2l/common/dialogs/quickLink/quickLink.d2l?ou=1&type=quiz", Some("courses.maine.edu"));
        assert_eq!(out, "https://courses.maine.edu/d2l/common/dialogs/quickLink/quickLink.d2l?ou=1&type=quiz");
    }

    #[test]
    fn absolute_url_untouched() {
        let out = absolutize_link("https://youtu.be/abc", Some("courses.maine.edu"));
        assert_eq!(out, "https://youtu.be/abc");
    }

    #[test]
    fn protocol_relative_untouched() {
        // `//host/path` is already host-qualified; prepending would corrupt it.
        let out = absolutize_link("//cdn.example.com/x", Some("courses.maine.edu"));
        assert_eq!(out, "//cdn.example.com/x");
    }

    #[test]
    fn relative_without_host_passes_through() {
        let out = absolutize_link("/d2l/x", None);
        assert_eq!(out, "/d2l/x");
    }

    #[test]
    fn shortcut_body_is_crlf_and_absolute() {
        let s = internet_shortcut("/d2l/x", Some("h.edu"));
        assert_eq!(s, "[InternetShortcut]\r\nURL=https://h.edu/d2l/x\r\n");
    }
}
