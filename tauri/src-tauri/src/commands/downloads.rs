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
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Serialize)]
pub struct DownloadBytes {
    pub bytes_base64: String,
    pub mime: Option<String>,
    pub filename: String,
}

#[tauri::command]
pub async fn download_topic_file(
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
    Ok(DownloadBytes {
        bytes_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        mime,
        filename,
    })
}

#[tauri::command]
pub async fn download_module_archive(
    state: AppStateArg<'_>,
    course_id: String,
    module_id: String,
) -> Result<DownloadBytes> {
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
    collect_module(&mut zip, &state, &course_id, &module_node, "", &mut seen, &mut failures).await?;

    if zip.entry_count() == 0 {
        return Err(AppError::Other("No downloadable files found in this module.".to_string()));
    }
    add_download_warnings(&mut zip, &failures);

    let bytes = zip.finish();
    let filename = format!("Brilliant-{}-{}.zip", course_id, sanitize(&module_title));
    Ok(DownloadBytes {
        bytes_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        mime: Some("application/zip".to_string()),
        filename,
    })
}

#[tauri::command]
pub async fn download_course_archive(
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<DownloadBytes> {
    let toc = state.client.get_toc(&state.pool, &course_id, false).await?;
    let modules = toc.get("Modules").and_then(|m| m.as_array()).cloned().unwrap_or_default();

    let mut zip = zip_writer::Builder::new();
    let mut seen = HashSet::new();
    let mut failures = Vec::new();
    for m in &modules {
        if let Err(e) = collect_module(&mut zip, &state, &course_id, m, "Table_of_Contents/", &mut seen, &mut failures).await {
            tracing::warn!("module skipped during course archive: {}", e);
            failures.push(format!("module {} skipped: {}", module_title(m), e));
        }
    }

    collect_assignment_files(&mut zip, &state, &course_id, &mut seen, &mut failures).await;
    collect_overview_attachments(&mut zip, &state, &course_id, &mut seen, &mut failures).await;

    // Best-effort: fetch the course overview attachment if one exists.
    let overview_path = format!("/d2l/api/le/{}/{}/overview", crate::client::API_VERSION, course_id);
    if let Ok(ov) = state.client.do_get(&state.pool, &overview_path, false).await {
        if let Some(att_url) = ov
            .pointer("/Attachment/Url")
            .and_then(|v| v.as_str())
            .or_else(|| ov.pointer("/Attachment/Href").and_then(|v| v.as_str()))
        {
            match state.client.fetch_bytes(att_url).await {
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

    if zip.entry_count() == 0 {
        return Err(AppError::Other("No downloadable files found in this course.".to_string()));
    }
    add_download_warnings(&mut zip, &failures);

    let bytes = zip.finish();
    Ok(DownloadBytes {
        bytes_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        mime: Some("application/zip".to_string()),
        filename: format!("Brilliant-{}.zip", course_id),
    })
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
    state: &AppStateArg<'_>,
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
                    zip.add_file(&entry, internet_shortcut(url).as_bytes());
                    continue;
                }
            }
            let path = topic_download_path(course_id, &topic_id);
            match state.client.fetch_bytes(&path).await {
                Ok((bytes, _mime, name)) => {
                    let fname = name.unwrap_or_else(|| filename_with_extension(&topic_title, None));
                    let entry = unique_path(seen, &format!("{}{}", folder, fname));
                    zip.add_file(&entry, &bytes);
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
    state: &AppStateArg<'_>,
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
    state: &AppStateArg<'_>,
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
    state: &AppStateArg<'_>,
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
    state: &AppStateArg<'_>,
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
        zip.add_file(&entry, internet_shortcut(&url).as_bytes());
        return;
    }

    match state.client.fetch_bytes(&url).await {
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

fn internet_shortcut(url: &str) -> String {
    format!("[InternetShortcut]\r\nURL={}\r\n", url)
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
