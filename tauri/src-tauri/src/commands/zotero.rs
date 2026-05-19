//! Tauri commands wrapping the Zotero client. Four entry points
//! (topic, module, course, syllabus) — all push files into a per-course
//! Zotero collection (auto-created), and shape each upload as a parent
//! item plus a child attachment.

use super::AppStateArg;
use crate::error::{AppError, Result};
use crate::zotero::{ItemType, ParentItem, ZoteroClient, ZoteroMode};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

const ZOTERO_EVENT: &str = "zotero://result";

#[derive(Debug, Clone, Serialize)]
pub struct ZoteroResult {
    /// Items successfully created in Zotero (attachment keys).
    pub created: Vec<String>,
    /// Per-item failures (human-readable). One archive can have several
    /// without aborting the whole batch.
    pub failures: Vec<String>,
    /// Zotero collection key the items landed in (so the UI can link to it).
    pub collection_key: Option<String>,
}

fn emit_result(app: &AppHandle, result: &ZoteroResult) {
    if let Err(e) = app.emit(ZOTERO_EVENT, result) {
        tracing::warn!("emit {} failed: {}", ZOTERO_EVENT, e);
    }
}

async fn load_mode(state: &AppStateArg<'_>) -> Result<ZoteroMode> {
    // In local mode, all four credential fields are optional and serve
    // different purposes:
    //   - zotero_basic_auth_user / pass → HTTP Basic Auth for the
    //     reverse proxy (nginx htpasswd / similar)
    //   - zotero_user_id → the Zotero user ID for path /users/<id>/
    //     (or zotero_local_user_id overrides for proxies that need it)
    //   - zotero_api_key → Zotero's own API key, sent as Zotero-API-Key.
    //     Vanilla localhost Zotero ignores it; proxies that forward to
    //     api.zotero.org need it.
    let row: Option<(
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT zotero_use_local, zotero_user_id, zotero_api_key, zotero_local_base_url, zotero_local_user_id, zotero_basic_auth_user, zotero_basic_auth_pass FROM user_preferences LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await?;
    let (use_local_int, uid, key, local_base, local_user_id, ba_user, ba_pass) =
        row.unwrap_or((Some(1), None, None, None, None, None, None));
    let use_local = use_local_int.unwrap_or(1) != 0;
    if use_local {
        let basic_auth = match (ba_user.as_ref(), ba_pass.as_ref()) {
            (Some(u), Some(p)) if !u.trim().is_empty() && !p.trim().is_empty() => {
                Some((u.clone(), p.clone()))
            }
            _ => None,
        };
        return Ok(ZoteroMode::Local {
            base_url: local_base.filter(|s| !s.trim().is_empty()),
            basic_auth,
            user_id_override: local_user_id.filter(|s| !s.trim().is_empty()),
            api_key: key.filter(|s| !s.trim().is_empty()),
        });
    }
    match (uid, key) {
        (Some(u), Some(k)) if !u.trim().is_empty() && !k.trim().is_empty() => {
            Ok(ZoteroMode::Cloud { user_id: u, api_key: k })
        }
        _ => Err(AppError::Other(
            "Cloud Zotero is selected but user ID + API key are missing (Settings → Zotero).".into(),
        )),
    }
}

async fn course_display(state: &AppStateArg<'_>, course_id: &str) -> Result<(String, Option<String>)> {
    let row: Option<(String, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT name, custom_name, code, custom_code FROM courses WHERE org_unit_id = ?",
    )
    .bind(course_id)
    .fetch_optional(&state.pool)
    .await?;
    let row = row.ok_or_else(|| AppError::Other(format!("course {} not found", course_id)))?;
    let title = row.1.filter(|s| !s.is_empty()).unwrap_or(row.0);
    let code = row.3.or(row.2);
    Ok((title, code))
}

fn collection_name(course_title: &str, code: Option<&str>) -> String {
    match code {
        Some(c) if !c.is_empty() => format!("{} — {}", c, course_title),
        _ => course_title.to_string(),
    }
}

fn brightspace_host(state: &AppStateArg<'_>) -> Option<String> {
    state.client.host_clone()
}

fn topic_view_url(host: &str, course_id: &str, topic_id: &str) -> String {
    // Standard Brightspace deep-link to a content topic.
    format!("https://{}/d2l/le/content/{}/viewContent/{}/View", host, course_id, topic_id)
}

fn topic_download_path(course_id: &str, topic_id: &str) -> String {
    format!(
        "/d2l/api/le/{}/{}/content/topics/{}/file",
        crate::client::API_VERSION,
        course_id,
        topic_id,
    )
}

fn module_id_str(m: &Value) -> Option<String> {
    m.get("Identifier")
        .or_else(|| m.get("ModuleId"))
        .or_else(|| m.get("Id"))
        .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))
}

fn topic_id_str(t: &Value) -> Option<String> {
    t.get("Identifier")
        .or_else(|| t.get("TopicId"))
        .or_else(|| t.get("Id"))
        .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))
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

/// Try to ensure a collection but tolerate Zotero servers that don't
/// expose /collections (the local HTTP API is a limited subset of the
/// Web API). On 403/404 we return None and the caller drops items into
/// the root library tagged with the course code instead — the user can
/// re-file in Zotero, but at least Send to Zotero stops 403'ing.
async fn try_ensure_collection(
    zc: &ZoteroClient,
    name: &str,
    parent: Option<&str>,
) -> Option<String> {
    match zc.ensure_collection(name, parent).await {
        Ok(k) => Some(k),
        Err(e) => {
            tracing::warn!(
                "zotero collection unavailable for '{}'; sending to root library: {}",
                name,
                e
            );
            None
        }
    }
}

/// Topics in a Brightspace TOC come in many flavors — File (PDF/Word/etc),
/// Link (external URL), Quiz, Discussion, Dropbox (assignment), Survey,
/// SCORM, etc. Only the File flavor belongs in Zotero; the rest fetch as
/// HTML stubs of the Brightspace page and just pollute the library.
///
/// D2L's TypeIdentifier strings vary across versions and we don't want to
/// hard-code an exhaustive list, so the check is positive — accept only
/// when the type hint clearly says "File" / numeric Type=1, OR when the
/// URL points at a known doc extension. Anything else is skipped silently.
fn is_zotero_eligible_topic(topic: &Value) -> bool {
    let type_hint = topic
        .get("TypeIdentifier")
        .or_else(|| topic.get("Type"))
        .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string())))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if type_hint == "file" || type_hint == "1" {
        return true;
    }
    if type_hint.contains("link")
        || type_hint.contains("quiz")
        || type_hint.contains("discussion")
        || type_hint.contains("dropbox")
        || type_hint.contains("survey")
        || type_hint.contains("scorm")
        || type_hint.contains("checklist")
    {
        return false;
    }
    // Fallback: trust the URL's extension if Brightspace omitted a
    // TypeIdentifier we recognize.
    let url = topic.get("Url").and_then(|v| v.as_str()).unwrap_or_default().to_ascii_lowercase();
    let exts = [
        ".pdf", ".doc", ".docx", ".ppt", ".pptx", ".xls", ".xlsx",
        ".odt", ".rtf", ".txt", ".md", ".csv",
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".heic",
        ".epub", ".mobi",
    ];
    exts.iter().any(|e| url.ends_with(e))
}

/// After fetching bytes, accept only mime types Zotero actually handles
/// as files. text/html means we got the Brightspace UI page (link stub)
/// rather than a downloadable file — skip those even if the topic looked
/// like a candidate.
fn is_zotero_eligible_mime(mime: Option<&str>) -> bool {
    let Some(m) = mime else { return true };
    let m = m.to_ascii_lowercase();
    if m.starts_with("text/html") {
        return false;
    }
    true
}

async fn send_topic_to_zotero(
    zc: &ZoteroClient,
    state: &AppStateArg<'_>,
    course_id: &str,
    course_title: &str,
    course_code: Option<&str>,
    collection_key: Option<&str>,
    topic: &Value,
    result: &mut ZoteroResult,
) {
    if !is_zotero_eligible_topic(topic) {
        return; // quiz/link/discussion/etc — silently skip
    }
    let Some(topic_id) = topic_id_str(topic) else { return };
    let topic_title = topic
        .get("Title")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("topic_{}", topic_id));

    let bytes_path = topic_download_path(course_id, &topic_id);
    let (bytes, mime, response_name) = match state.client.fetch_bytes(&bytes_path).await {
        Ok(t) => t,
        Err(e) => {
            result.failures.push(format!("'{}': {}", topic_title, e));
            return;
        }
    };
    // Defensive: Brightspace sometimes returns an HTML stub even for
    // topics we thought were Files (e.g. removed content). Skip those —
    // they'd land as garbage HTML attachments in Zotero.
    if !is_zotero_eligible_mime(mime.as_deref()) {
        tracing::info!(
            "skipping '{}' for zotero — content-type {} not a document",
            topic_title,
            mime.as_deref().unwrap_or("?"),
        );
        return;
    }
    let filename = response_name.unwrap_or_else(|| sanitize(&topic_title));
    let our_md5 = crate::zotero::ZoteroClient::md5_of(&bytes);

    // Stable identifier the next send-to-Zotero can use to recognise this
    // topic. Course + topic id is unique across Brightspace; including the
    // course id lets you re-send the same topic across courses without
    // colliding (unlikely but cheap).
    let bsid_tag = format!("brilliant:bsid={}:{}", course_id, topic_id);

    // Look for a previous send. Two outcomes guide what happens:
    //   - found + same MD5         → skip silently (idempotent re-send)
    //   - found + different MD5    → replace the file on the existing
    //                                attachment (newer Brightspace upload)
    //   - not found                → fall through to create + upload
    match zc.find_by_brilliant_tag(&bsid_tag).await {
        Ok(Some(existing)) => {
            if existing.attachment_md5.as_deref() == Some(our_md5.as_str()) {
                tracing::info!(
                    "zotero: '{}' already current (md5 match); skipping",
                    topic_title
                );
                return;
            }
            if let Some(attachment_key) = existing.attachment_key.as_deref() {
                match zc
                    .replace_attachment_file(attachment_key, &filename, mime.as_deref(), &bytes)
                    .await
                {
                    Ok(()) => {
                        result.created.push(attachment_key.to_string());
                        return;
                    }
                    Err(e) => {
                        result.failures.push(format!(
                            "'{}': replace file on existing item failed: {}",
                            topic_title, e
                        ));
                        return;
                    }
                }
            }
            // Parent existed but had no attachment yet — attach now.
            match zc
                .upload_attachment(&existing.key, &filename, mime.as_deref(), &bytes)
                .await
            {
                Ok(k) => result.created.push(k),
                Err(e) => result.failures.push(format!("'{}': upload to existing parent: {}", topic_title, e)),
            }
            return;
        }
        Ok(None) => { /* fall through */ }
        Err(e) => {
            // Tag search failing isn't fatal — log and treat as "not found"
            // so we still ship the item. Worst case the user gets a dup.
            tracing::warn!("zotero tag lookup failed for '{}'; sending anyway: {}", topic_title, e);
        }
    }

    let host = brightspace_host(state);
    let mut tags = vec![
        format!("Brilliant: {}", course_title),
        bsid_tag.clone(),
    ];
    if let Some(c) = course_code {
        tags.push(c.to_string());
    }
    let parent = ParentItem {
        item_type: ItemType::Document,
        title: topic_title.clone(),
        url: host.as_ref().map(|h| topic_view_url(h, course_id, &topic_id)),
        tags,
        collection_keys: collection_key.map(|k| vec![k.to_string()]).unwrap_or_default(),
    };
    let parent_key = match zc.create_parent_item(&parent).await {
        Ok(k) => k,
        Err(e) => {
            result.failures.push(format!("'{}': create parent: {}", topic_title, e));
            return;
        }
    };
    match zc
        .upload_attachment(&parent_key, &filename, mime.as_deref(), &bytes)
        .await
    {
        Ok(k) => result.created.push(k),
        Err(e) => result.failures.push(format!("'{}': upload: {}", topic_title, e)),
    }
}

async fn walk_module_for_zotero(
    zc: &ZoteroClient,
    state: &AppStateArg<'_>,
    course_id: &str,
    course_title: &str,
    course_code: Option<&str>,
    collection_key: Option<&str>,
    node: &Value,
    result: &mut ZoteroResult,
) {
    if let Some(topics) = node.get("Topics").and_then(|v| v.as_array()) {
        for t in topics {
            send_topic_to_zotero(
                zc, state, course_id, course_title, course_code, collection_key, t, result,
            )
            .await;
        }
    }
    if let Some(subs) = node.get("Modules").and_then(|v| v.as_array()) {
        for sub in subs {
            Box::pin(walk_module_for_zotero(
                zc, state, course_id, course_title, course_code, collection_key, sub, result,
            ))
            .await;
        }
    }
}

#[tauri::command]
pub async fn zotero_send_topic(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
    topic_id: String,
) -> Result<ZoteroResult> {
    let mode = load_mode(&state).await?;
    let zc = ZoteroClient::new(mode)?;
    let (title, code) = course_display(&state, &course_id).await?;
    let collection_key =
        try_ensure_collection(&zc, &collection_name(&title, code.as_deref()), None).await;
    let toc = state.client.get_toc(&state.pool, &course_id, false).await?;
    // Find the topic anywhere in the TOC. Simplest: walk every module.
    let mut topic_value: Option<Value> = None;
    if let Some(mods) = toc.get("Modules").and_then(|v| v.as_array()) {
        for m in mods {
            if let Some(found) = find_topic_recursive(m, &topic_id) {
                topic_value = Some(found);
                break;
            }
        }
    }
    let topic = topic_value
        .ok_or_else(|| AppError::Other(format!("topic {} not found in TOC", topic_id)))?;
    let mut result = ZoteroResult {
        created: Vec::new(),
        failures: Vec::new(),
        collection_key: collection_key.clone(),
    };
    send_topic_to_zotero(
        &zc,
        &state,
        &course_id,
        &title,
        code.as_deref(),
        collection_key.as_deref(),
        &topic,
        &mut result,
    )
    .await;
    emit_result(&app, &result);
    Ok(result)
}

fn find_topic_recursive(node: &Value, target: &str) -> Option<Value> {
    if let Some(topics) = node.get("Topics").and_then(|v| v.as_array()) {
        for t in topics {
            if topic_id_str(t).as_deref() == Some(target) {
                return Some(t.clone());
            }
        }
    }
    if let Some(subs) = node.get("Modules").and_then(|v| v.as_array()) {
        for sub in subs {
            if let Some(found) = find_topic_recursive(sub, target) {
                return Some(found);
            }
        }
    }
    None
}

#[tauri::command]
pub async fn zotero_send_module(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
    module_id: String,
) -> Result<ZoteroResult> {
    let mode = load_mode(&state).await?;
    let zc = ZoteroClient::new(mode)?;
    let (title, code) = course_display(&state, &course_id).await?;
    let course_collection =
        try_ensure_collection(&zc, &collection_name(&title, code.as_deref()), None).await;
    let toc = state.client.get_toc(&state.pool, &course_id, false).await?;
    let module = find_module(&toc, &module_id)
        .ok_or_else(|| AppError::Other(format!("module {} not found in TOC", module_id)))?;
    let module_title = module
        .get("Title")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("Module {}", module_id));
    // Each module gets its own sub-collection under the course — only if
    // the server supported the parent lookup. Otherwise drop to root.
    let module_collection = match course_collection.as_deref() {
        Some(parent) => try_ensure_collection(&zc, &module_title, Some(parent)).await,
        None => None,
    };
    let mut result = ZoteroResult {
        created: Vec::new(),
        failures: Vec::new(),
        collection_key: module_collection.clone().or(course_collection.clone()),
    };
    walk_module_for_zotero(
        &zc,
        &state,
        &course_id,
        &title,
        code.as_deref(),
        module_collection.as_deref().or(course_collection.as_deref()),
        &module,
        &mut result,
    )
    .await;
    emit_result(&app, &result);
    Ok(result)
}

#[tauri::command]
pub async fn zotero_send_course(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<ZoteroResult> {
    let mode = load_mode(&state).await?;
    let zc = ZoteroClient::new(mode)?;
    let (title, code) = course_display(&state, &course_id).await?;
    let course_collection =
        try_ensure_collection(&zc, &collection_name(&title, code.as_deref()), None).await;
    let toc = state.client.get_toc(&state.pool, &course_id, false).await?;
    let mut result = ZoteroResult {
        created: Vec::new(),
        failures: Vec::new(),
        collection_key: course_collection.clone(),
    };
    if let Some(modules) = toc.get("Modules").and_then(|v| v.as_array()) {
        for m in modules {
            let module_title = m
                .get("Title")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| "Untitled module".into());
            // Sub-collection per module — only if the server supported the
            // course-level collection lookup. If it didn't (local Zotero
            // 403s on /collections), items go to the root tagged with the
            // course code.
            let module_collection = match course_collection.as_deref() {
                Some(parent) => try_ensure_collection(&zc, &module_title, Some(parent)).await,
                None => None,
            };
            walk_module_for_zotero(
                &zc,
                &state,
                &course_id,
                &title,
                code.as_deref(),
                module_collection.as_deref().or(course_collection.as_deref()),
                m,
                &mut result,
            )
            .await;
        }
    }
    emit_result(&app, &result);
    Ok(result)
}

#[tauri::command]
pub async fn zotero_send_syllabus(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<ZoteroResult> {
    let mode = load_mode(&state).await?;
    let zc = ZoteroClient::new(mode)?;
    let (title, code) = course_display(&state, &course_id).await?;
    let collection_key =
        try_ensure_collection(&zc, &collection_name(&title, code.as_deref()), None).await;
    let overview = state.client.get_overview(&state.pool, &course_id, false).await?;
    let has_attachment = overview
        .get("HasAttachment")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || overview.pointer("/Attachment/Url").is_some();
    if !has_attachment {
        return Err(AppError::Other(
            "This course's overview has no attachment to send.".into(),
        ));
    }
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
    let our_md5 = crate::zotero::ZoteroClient::md5_of(&bytes);
    let bsid_tag = format!("brilliant:syllabus={}", course_id);

    // Idempotent: re-sending the same syllabus reuses the existing parent
    // and only replaces the file when Brightspace's copy has changed.
    if let Ok(Some(existing)) = zc.find_by_brilliant_tag(&bsid_tag).await {
        if existing.attachment_md5.as_deref() == Some(our_md5.as_str()) {
            let result = ZoteroResult {
                created: Vec::new(),
                failures: Vec::new(),
                collection_key,
            };
            emit_result(&app, &result);
            return Ok(result);
        }
        if let Some(attachment_key) = existing.attachment_key.as_deref() {
            zc.replace_attachment_file(attachment_key, &filename, mime.as_deref(), &bytes)
                .await?;
            let result = ZoteroResult {
                created: vec![attachment_key.to_string()],
                failures: Vec::new(),
                collection_key,
            };
            emit_result(&app, &result);
            return Ok(result);
        }
    }

    let mut tags = vec![format!("Brilliant: {}", title), bsid_tag];
    if let Some(c) = code.as_deref() {
        tags.push(c.to_string());
    }
    tags.push("syllabus".to_string());
    let parent = ParentItem {
        item_type: ItemType::Document,
        title: format!("Syllabus — {}", title),
        url: brightspace_host(&state).map(|h| format!("https://{}/d2l/home/{}", h, course_id)),
        tags,
        collection_keys: collection_key.clone().map(|k| vec![k]).unwrap_or_default(),
    };
    let parent_key = zc.create_parent_item(&parent).await?;
    let attachment_key = zc
        .upload_attachment(&parent_key, &filename, mime.as_deref(), &bytes)
        .await?;
    let result = ZoteroResult {
        created: vec![attachment_key],
        failures: Vec::new(),
        collection_key,
    };
    emit_result(&app, &result);
    Ok(result)
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "untitled".into()
    } else {
        trimmed.to_string()
    }
}
