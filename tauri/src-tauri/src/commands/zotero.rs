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
    /// Items already in Zotero carrying the same file, so there was nothing
    /// to do. Counted apart from `created` because "everything is current"
    /// and "there was nothing here to send" are opposite outcomes and the
    /// UI was reporting both as the latter.
    pub up_to_date: usize,
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

/// File an item that already exists into the collection this send targets.
///
/// An item's collections are written only at creation, so without this a
/// re-send leaves the item wherever it already was. Deleting a collection in
/// Zotero does not delete its items, so the folder would come back empty on
/// the next send while every item reported "already up to date" — the send
/// looked successful and changed nothing.
///
/// Best-effort: the content is already in Zotero, so a re-file that fails is
/// a warning, not a failed send.
async fn refile_existing(
    zc: &ZoteroClient,
    item_key: &str,
    collection_key: Option<&str>,
    label: &str,
) {
    let Some(ck) = collection_key else { return };
    match zc.add_to_collection(item_key, ck).await {
        Ok(true) => tracing::info!("zotero: re-filed '{}' into collection {}", label, ck),
        Ok(false) => {} // already there — the normal case
        Err(e) => tracing::warn!(
            "zotero: could not re-file '{}' into collection {}: {}",
            label,
            ck,
            e
        ),
    }
}

/// Where a send files its items, resolved lazily.
///
/// Collections used to be created up front — one for the course, then one
/// per module before we knew whether the module had anything in it. Modules
/// full of quizzes, links, or content pages left empty collections behind,
/// and a re-send of an unchanged course created them again. Nothing is
/// created here until a topic is actually about to be written, so a send
/// that files nothing touches the library's structure not at all.
///
/// `Option<Option<String>>` throughout: the outer layer is "have we tried
/// yet", the inner is "did the server give us one" — some Zotero servers
/// don't expose /collections at all, and there we fall back to the root.
struct CollectionTarget<'a> {
    zc: &'a ZoteroClient,
    course_name: String,
    module_name: Option<String>,
    course_key: Option<Option<String>>,
    module_key: Option<Option<String>>,
}

impl<'a> CollectionTarget<'a> {
    fn new(zc: &'a ZoteroClient, course_name: String) -> Self {
        Self { zc, course_name, module_name: None, course_key: None, module_key: None }
    }

    /// Point subsequent items at a module sub-collection. The course-level
    /// resolution is deliberately kept, so walking twelve modules still
    /// resolves the course collection once.
    fn enter_module(&mut self, module_title: &str) {
        self.module_name = Some(module_title.to_string());
        self.module_key = None;
    }

    /// The collection an item should go into, creating it if this is the
    /// first item that needs it. None means the library root.
    async fn resolve(&mut self) -> Option<String> {
        let course = self.resolve_course().await;
        let Some(module_name) = self.module_name.clone() else {
            return course;
        };
        // Without a course collection there's nothing to nest under, so the
        // module collection is skipped rather than created at the top level.
        let parent = course.clone()?;
        if self.module_key.is_none() {
            self.module_key =
                Some(try_ensure_collection(self.zc, &module_name, Some(&parent)).await);
        }
        self.module_key.clone().flatten().or(course)
    }

    async fn resolve_course(&mut self) -> Option<String> {
        if self.course_key.is_none() {
            self.course_key = Some(try_ensure_collection(self.zc, &self.course_name, None).await);
        }
        self.course_key.clone().flatten()
    }

    /// The collection the UI should link to — only whatever we actually
    /// ended up resolving, so a send that filed nothing reports nothing.
    fn reported_key(&self) -> Option<String> {
        self.module_key
            .clone()
            .flatten()
            .or_else(|| self.course_key.clone().flatten())
    }

    /// As `reported_key`, but never a module sub-collection — for the
    /// whole-course send, where the course collection is the useful link.
    fn reported_course_key(&self) -> Option<String> {
        self.course_key.clone().flatten()
    }
}

/// Topics in a Brightspace TOC come in many flavors — File (PDF/Word/etc),
/// Link (external URL), Quiz, Discussion, Dropbox (assignment), Survey,
/// SCORM, etc. Only ones with a document behind them belong in Zotero.
///
/// This used to be a *positive* filter: accept only `TypeIdentifier` of
/// "File"/1, or a URL ending in a known extension. That silently dropped
/// real course documents — anything Brightspace reports under a type string
/// we didn't enumerate, which includes files stored through the newer
/// content service, whose URLs carry no extension either. So the check is
/// now negative: reject the flavors that definitionally have no file, and
/// attempt the rest. The authoritative "is this a document" test is the
/// content type of what actually comes back, which is a fact rather than a
/// guess — see `fetch_topic_document`.
fn is_zotero_eligible_topic(topic: &Value) -> bool {
    let type_hint = topic
        .get("TypeIdentifier")
        .or_else(|| topic.get("Type"))
        .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string())))
        .unwrap_or_default()
        .to_ascii_lowercase();
    !(type_hint.contains("link")
        || type_hint.contains("quiz")
        || type_hint.contains("discussion")
        || type_hint.contains("dropbox")
        || type_hint.contains("survey")
        || type_hint.contains("scorm")
        || type_hint.contains("checklist"))
}

/// Outcome of trying to get a topic's actual document bytes.
enum TopicFetch {
    Document {
        bytes: Vec<u8>,
        mime: Option<String>,
        filename: Option<String>,
    },
    /// There is no document here — skip quietly, but say why in the log.
    NotADocument(String),
    /// There should have been a document and we couldn't get it.
    Failed(String),
}

/// Resolve a topic to the file itself.
///
/// `/content/topics/<id>/file` is the correct endpoint for a stored file,
/// but Brightspace will happily answer it with the HTML of the viewContent
/// page for topics it doesn't treat as directly downloadable. That HTML is
/// the page *about* the document, not the document — sending it to Zotero is
/// what produced library entries that were nothing but a Brightspace link.
///
/// So: try the API endpoint, and whenever it gives us HTML (or fails
/// outright), fall back to the topic's own `Url`, which for uploaded course
/// files is the direct `/content/enforced/<course>/<file>` path.
async fn fetch_topic_document(
    state: &AppStateArg<'_>,
    course_id: &str,
    topic_id: &str,
    topic: &Value,
) -> TopicFetch {
    let api_path = topic_download_path(course_id, topic_id);
    match state.client.fetch_bytes(&api_path).await {
        Ok((bytes, mime, name)) if is_zotero_eligible_mime(mime.as_deref()) => {
            TopicFetch::Document { bytes, mime, filename: name }
        }
        Ok((_, mime, _)) => match fetch_topic_direct_url(state, topic).await {
            Some(Ok((bytes, direct_mime, name))) if is_zotero_eligible_mime(direct_mime.as_deref()) => {
                TopicFetch::Document { bytes, mime: direct_mime, filename: name }
            }
            Some(Ok(_)) | None => TopicFetch::NotADocument(format!(
                "Brightspace served the content page ({}), not a file",
                mime.as_deref().unwrap_or("no content-type"),
            )),
            Some(Err(e)) => TopicFetch::Failed(e),
        },
        Err(e) => match fetch_topic_direct_url(state, topic).await {
            Some(Ok((bytes, mime, name))) if is_zotero_eligible_mime(mime.as_deref()) => {
                TopicFetch::Document { bytes, mime, filename: name }
            }
            Some(Ok(_)) => TopicFetch::NotADocument(
                "the topic's own URL is a Brightspace page, not a file".to_string(),
            ),
            Some(Err(direct)) => TopicFetch::Failed(format!("{} (direct URL: {})", e, direct)),
            None => TopicFetch::Failed(e.to_string()),
        },
    }
}

/// Fetch a topic's own `Url`. `None` when there's nothing usable to try:
/// no URL, or an absolute one pointing off-site (an external link topic —
/// we're not pulling arbitrary web pages into the user's library).
#[allow(clippy::type_complexity)]
async fn fetch_topic_direct_url(
    state: &AppStateArg<'_>,
    topic: &Value,
) -> Option<std::result::Result<(Vec<u8>, Option<String>, Option<String>), String>> {
    let url = topic.get("Url").and_then(|v| v.as_str())?.trim();
    if url.is_empty() || url.starts_with("http://") || url.starts_with("https://") {
        return None;
    }
    Some(
        state
            .client
            .fetch_bytes(url)
            .await
            .map_err(|e| e.to_string()),
    )
}

/// Zotero rejects an attachment filename that isn't a bare name — it gets
/// joined onto the storage directory path — so the server-suggested name
/// from `Content-Disposition` goes through the same sanitizer as our own
/// titles. A name with no extension also leaves Zotero guessing at how to
/// open the file, so borrow one from the content type when we can.
fn attachment_filename(
    response_name: Option<String>,
    topic_title: &str,
    mime: Option<&str>,
) -> String {
    let base = sanitize(&response_name.unwrap_or_else(|| topic_title.to_string()));
    if std::path::Path::new(&base).extension().is_some() {
        return base;
    }
    match extension_for_mime(mime) {
        Some(ext) => format!("{}{}", base, ext),
        None => base,
    }
}

fn extension_for_mime(mime: Option<&str>) -> Option<&'static str> {
    // Content types arrive with parameters attached ("application/pdf;
    // charset=binary"), so match on the bare type.
    let m = mime?.split(';').next()?.trim().to_ascii_lowercase();
    Some(match m.as_str() {
        "application/pdf" => ".pdf",
        "application/msword" => ".doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => ".docx",
        "application/vnd.ms-powerpoint" => ".ppt",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => ".pptx",
        "application/vnd.ms-excel" => ".xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => ".xlsx",
        "application/vnd.oasis.opendocument.text" => ".odt",
        "application/rtf" | "text/rtf" => ".rtf",
        "application/epub+zip" => ".epub",
        "application/zip" => ".zip",
        "text/plain" => ".txt",
        "text/csv" => ".csv",
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        _ => return None,
    })
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
    target: &mut CollectionTarget<'_>,
    topic: &Value,
    result: &mut ZoteroResult,
) -> Option<String> {
    if !is_zotero_eligible_topic(topic) {
        return None; // quiz/link/discussion/etc — silently skip
    }
    let topic_id = topic_id_str(topic)?;
    let topic_title = topic
        .get("Title")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("topic_{}", topic_id));

    let (bytes, mime, response_name) =
        match fetch_topic_document(state, course_id, &topic_id, topic).await {
            TopicFetch::Document { bytes, mime, filename } => (bytes, mime, filename),
            TopicFetch::NotADocument(why) => {
                tracing::info!("zotero: skipping '{}' — {}", topic_title, why);
                return None;
            }
            TopicFetch::Failed(why) => {
                result.failures.push(format!("'{}': {}", topic_title, why));
                return None;
            }
        };
    let filename = attachment_filename(response_name, &topic_title, mime.as_deref());
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
            // Resolve the collection before any of the early returns below.
            // Every branch here ends with the item still in the library but
            // possibly filed nowhere this send knows about, and only the
            // create path further down passes `collections` in the payload.
            // Resolving here does not manufacture empty collections: we only
            // reach this point for a topic that is Zotero-eligible and has a
            // document, which is exactly an item that belongs in the folder.
            let existing_collection = target.resolve().await;
            refile_existing(zc, &existing.key, existing_collection.as_deref(), &topic_title).await;

            if existing.attachment_md5.as_deref() == Some(our_md5.as_str()) {
                tracing::info!(
                    "zotero: '{}' already current (md5 match); skipping",
                    topic_title
                );
                result.up_to_date += 1;
                return Some(existing.key);
            }
            if let Some(attachment_key) = existing.attachment_key.as_deref() {
                match zc
                    .replace_attachment_file(attachment_key, &filename, mime.as_deref(), &bytes)
                    .await
                {
                    Ok(()) => {
                        result.created.push(attachment_key.to_string());
                        return Some(existing.key);
                    }
                    Err(e) => {
                        result.failures.push(format!(
                            "'{}': replace file on existing item failed: {}",
                            topic_title, e
                        ));
                        return None;
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
            return Some(existing.key);
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
    // First item that actually needs a home — resolve (and create) the
    // collection now, not when the walk started.
    let collection_key = target.resolve().await;
    let parent = ParentItem {
        item_type: ItemType::Document,
        title: topic_title.clone(),
        url: host.as_ref().map(|h| topic_view_url(h, course_id, &topic_id)),
        tags,
        collection_keys: collection_key.map(|k| vec![k]).unwrap_or_default(),
    };
    let parent_key = match zc.create_parent_item(&parent).await {
        Ok(k) => k,
        Err(e) => {
            result.failures.push(format!("'{}': create parent: {}", topic_title, e));
            return None;
        }
    };
    match zc
        .upload_attachment(&parent_key, &filename, mime.as_deref(), &bytes)
        .await
    {
        Ok(k) => {
            result.created.push(k);
            return Some(parent_key);
        }
        Err(e) => {
            // Roll the parent back. Leaving it behind is the other half of
            // the "Zotero entry is just a Brightspace URL" problem: the item
            // exists, carries the viewContent link, and has no document
            // under it, which reads as though we deliberately filed the page
            // instead of the file.
            if let Err(cleanup) = zc.delete_item(&parent_key).await {
                tracing::warn!(
                    "zotero: '{}' upload failed and the stub item {} could not be removed: {}",
                    topic_title,
                    parent_key,
                    cleanup,
                );
            }
            result.failures.push(format!("'{}': upload: {}", topic_title, e));
            None
        }
    }
}

async fn walk_module_for_zotero(
    zc: &ZoteroClient,
    state: &AppStateArg<'_>,
    course_id: &str,
    course_title: &str,
    course_code: Option<&str>,
    target: &mut CollectionTarget<'_>,
    node: &Value,
    result: &mut ZoteroResult,
    touched: &mut Vec<String>,
) {
    if let Some(topics) = node.get("Topics").and_then(|v| v.as_array()) {
        for t in topics {
            if let Some(key) = send_topic_to_zotero(
                zc, state, course_id, course_title, course_code, target, t, result,
            )
            .await
            {
                touched.push(key);
            }
        }
    }
    if let Some(subs) = node.get("Modules").and_then(|v| v.as_array()) {
        for sub in subs {
            Box::pin(walk_module_for_zotero(
                zc, state, course_id, course_title, course_code, target, sub, result, touched,
            ))
            .await;
        }
    }
}

/// Cross-link every item that came out of one module so each shows the rest
/// under "Related" — the week's readings sit together when you come back to
/// write from them. Grouping is by top-level module, so a week's nested
/// sub-modules ("Required Readings", "Case Study") all count as one set.
///
/// Deliberately best-effort: a library that won't take relations is not a
/// reason to fail a send whose files all arrived. Failures are logged once
/// per group rather than pushed into `result.failures`.
async fn relate_module_items(zc: &ZoteroClient, keys: &[String], module_title: &str) {
    if keys.len() < 2 {
        return; // nothing to relate a lone item to
    }
    let uris: Vec<String> = keys.iter().map(|k| zc.item_uri(k)).collect();
    let mut linked = 0usize;
    let mut first_error: Option<String> = None;
    for key in keys {
        match zc.add_related(key, &uris).await {
            Ok(true) => linked += 1,
            Ok(false) => {}
            Err(e) => {
                if first_error.is_none() {
                    first_error = Some(e.to_string());
                }
            }
        }
    }
    match first_error {
        None if linked > 0 => tracing::info!(
            "zotero: related {} item(s) within '{}'",
            linked,
            module_title
        ),
        None => {}
        Some(e) => tracing::warn!(
            "zotero: could not relate items within '{}' ({} of {} updated): {}",
            module_title,
            linked,
            keys.len(),
            e,
        ),
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
    let zc = ZoteroClient::new(mode).await?;
    let (title, code) = course_display(&state, &course_id).await?;
    let mut target = CollectionTarget::new(&zc, collection_name(&title, code.as_deref()));
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
        collection_key: None,
        up_to_date: 0,
    };
    // A single-topic send has no sibling set to build, so it doesn't relate
    // anything; sending the module or the course does that.
    send_topic_to_zotero(
        &zc,
        &state,
        &course_id,
        &title,
        code.as_deref(),
        &mut target,
        &topic,
        &mut result,
    )
    .await;
    result.collection_key = target.reported_key();
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
    let zc = ZoteroClient::new(mode).await?;
    let (title, code) = course_display(&state, &course_id).await?;
    let mut target = CollectionTarget::new(&zc, collection_name(&title, code.as_deref()));
    let toc = state.client.get_toc(&state.pool, &course_id, false).await?;
    let module = find_module(&toc, &module_id)
        .ok_or_else(|| AppError::Other(format!("module {} not found in TOC", module_id)))?;
    let module_title = module
        .get("Title")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("Module {}", module_id));
    // Each module gets its own sub-collection under the course, created on
    // the first item that lands in it.
    target.enter_module(&module_title);
    let mut result = ZoteroResult {
        created: Vec::new(),
        failures: Vec::new(),
        collection_key: None,
        up_to_date: 0,
    };
    let mut touched: Vec<String> = Vec::new();
    walk_module_for_zotero(
        &zc,
        &state,
        &course_id,
        &title,
        code.as_deref(),
        &mut target,
        &module,
        &mut result,
        &mut touched,
    )
    .await;
    relate_module_items(&zc, &touched, &module_title).await;
    result.collection_key = target.reported_key();
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
    let zc = ZoteroClient::new(mode).await?;
    let (title, code) = course_display(&state, &course_id).await?;
    let mut target = CollectionTarget::new(&zc, collection_name(&title, code.as_deref()));
    let toc = state.client.get_toc(&state.pool, &course_id, false).await?;
    let mut result = ZoteroResult {
        created: Vec::new(),
        failures: Vec::new(),
        collection_key: None,
        up_to_date: 0,
    };
    if let Some(modules) = toc.get("Modules").and_then(|v| v.as_array()) {
        for m in modules {
            let module_title = m
                .get("Title")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| "Untitled module".into());
            // Sub-collection per module, created only once a file in it is
            // ready to be written. Modules that turn out to hold nothing but
            // quizzes, links, or content pages leave no trace.
            target.enter_module(&module_title);
            let mut touched: Vec<String> = Vec::new();
            walk_module_for_zotero(
                &zc,
                &state,
                &course_id,
                &title,
                code.as_deref(),
                &mut target,
                m,
                &mut result,
                &mut touched,
            )
            .await;
            // Relate within the week, not across the whole course — the point
            // is "these readings go together", which stops being true at the
            // course level.
            relate_module_items(&zc, &touched, &module_title).await;
        }
    }
    result.collection_key = target.reported_course_key();
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
    let zc = ZoteroClient::new(mode).await?;
    let (title, code) = course_display(&state, &course_id).await?;
    let mut target = CollectionTarget::new(&zc, collection_name(&title, code.as_deref()));
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
    if !is_zotero_eligible_mime(mime.as_deref()) {
        return Err(AppError::Other(format!(
            "The syllabus attachment came back as {}, not a document.",
            mime.as_deref().unwrap_or("an unknown type"),
        )));
    }
    let filename = attachment_filename(response_name, &suggested, mime.as_deref());
    let our_md5 = crate::zotero::ZoteroClient::md5_of(&bytes);
    let bsid_tag = format!("brilliant:syllabus={}", course_id);

    // Idempotent: re-sending the same syllabus reuses the existing parent
    // and only replaces the file when Brightspace's copy has changed.
    if let Ok(Some(existing)) = zc.find_by_brilliant_tag(&bsid_tag).await {
        // Same reasoning as the topic path: file the existing syllabus into
        // this send's collection before returning, or a deleted course
        // folder can never be repopulated.
        let existing_collection = target.resolve().await;
        refile_existing(&zc, &existing.key, existing_collection.as_deref(), &suggested).await;

        if existing.attachment_md5.as_deref() == Some(our_md5.as_str()) {
            let result = ZoteroResult {
                created: Vec::new(),
                failures: Vec::new(),
                collection_key: target.reported_key(),
                up_to_date: 1,
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
                collection_key: target.reported_key(),
                up_to_date: 0,
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
    let collection_key = target.resolve().await;
    let parent = ParentItem {
        item_type: ItemType::Document,
        title: format!("Syllabus — {}", title),
        url: brightspace_host(&state).map(|h| format!("https://{}/d2l/home/{}", h, course_id)),
        tags,
        collection_keys: collection_key.clone().map(|k| vec![k]).unwrap_or_default(),
    };
    let parent_key = zc.create_parent_item(&parent).await?;
    let attachment_key = match zc
        .upload_attachment(&parent_key, &filename, mime.as_deref(), &bytes)
        .await
    {
        Ok(k) => k,
        Err(e) => {
            // Same rollback as the per-topic path — never leave an item
            // behind that holds only the Brightspace URL.
            if let Err(cleanup) = zc.delete_item(&parent_key).await {
                tracing::warn!(
                    "zotero: syllabus upload failed and stub item {} could not be removed: {}",
                    parent_key,
                    cleanup,
                );
            }
            return Err(e);
        }
    };
    let result = ZoteroResult {
        created: vec![attachment_key],
        failures: Vec::new(),
        collection_key: target.reported_key(),
        up_to_date: 0,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zotero::ZoteroMode;
    use serde_json::json;

    async fn offline_client() -> ZoteroClient {
        // Never contacted: these tests only exercise the paths that avoid
        // touching the server, which is precisely the point of them.
        ZoteroClient::new(ZoteroMode::Local {
            base_url: None,
            basic_auth: None,
            user_id_override: None,
            api_key: None,
        })
        .await
        .expect("client")
    }

    #[tokio::test]
    async fn a_send_that_files_nothing_creates_no_collections() {
        // The regression: walking a module used to create its collection up
        // front, so modules of quizzes and links left empty collections
        // behind and every re-send made them again. Resolving is the only
        // thing that talks to Zotero, so a target that was never resolved
        // provably created nothing.
        let zc = offline_client().await;
        let mut target = CollectionTarget::new(&zc, "SWO-402 — Methods".into());
        target.enter_module("Week 1");
        target.enter_module("Week 2");
        assert_eq!(target.reported_key(), None);
        assert_eq!(target.reported_course_key(), None);
    }

    #[tokio::test]
    async fn entering_a_module_keeps_the_course_resolution() {
        // Twelve modules should still resolve the course collection once.
        let zc = offline_client().await;
        let mut target = CollectionTarget::new(&zc, "SWO-402 — Methods".into());
        target.course_key = Some(Some("COURSEKEY".into()));
        target.module_key = Some(Some("WEEK1KEY".into()));

        target.enter_module("Week 2");

        assert_eq!(target.course_key, Some(Some("COURSEKEY".into())));
        assert_eq!(target.module_key, None, "the new module must resolve on its own");
        assert_eq!(target.reported_course_key(), Some("COURSEKEY".into()));
    }

    #[tokio::test]
    async fn whole_course_result_links_the_course_not_the_last_module() {
        let zc = offline_client().await;
        let mut target = CollectionTarget::new(&zc, "SWO-402 — Methods".into());
        target.course_key = Some(Some("COURSEKEY".into()));
        target.module_key = Some(Some("WEEK7KEY".into()));

        assert_eq!(target.reported_key(), Some("WEEK7KEY".into()));
        assert_eq!(target.reported_course_key(), Some("COURSEKEY".into()));
    }

    #[tokio::test]
    async fn a_server_without_collections_reports_none_rather_than_failing() {
        // try_ensure_collection swallows the error and records the attempt;
        // items then go to the library root.
        let zc = offline_client().await;
        let mut target = CollectionTarget::new(&zc, "SWO-402 — Methods".into());
        target.course_key = Some(None);
        assert_eq!(target.reported_key(), None);
        assert_eq!(target.reported_course_key(), None);
    }

    #[test]
    fn rejects_topics_with_no_file_behind_them() {
        for t in ["Link", "Quiz", "Discussion", "Dropbox", "Survey", "SCORM", "Checklist"] {
            assert!(
                !is_zotero_eligible_topic(&json!({ "TypeIdentifier": t })),
                "{t} should be skipped",
            );
        }
    }

    #[test]
    fn attempts_topics_whose_type_we_dont_recognize() {
        // The regression this guards: content-service files report neither
        // "File" nor a URL with an extension, and the old positive filter
        // dropped them without a word.
        assert!(is_zotero_eligible_topic(&json!({
            "TypeIdentifier": "ContentService",
            "Url": "/d2l/le/content/12345/viewContent/67890/View"
        })));
        assert!(is_zotero_eligible_topic(&json!({ "TypeIdentifier": "File" })));
        assert!(is_zotero_eligible_topic(&json!({})));
    }

    #[test]
    fn html_is_never_a_document() {
        assert!(!is_zotero_eligible_mime(Some("text/html")));
        assert!(!is_zotero_eligible_mime(Some("text/html; charset=utf-8")));
        assert!(is_zotero_eligible_mime(Some("application/pdf")));
        assert!(is_zotero_eligible_mime(None));
    }

    #[test]
    fn filename_gets_an_extension_from_the_content_type() {
        assert_eq!(
            attachment_filename(None, "Week 3 Reading", Some("application/pdf")),
            "Week 3 Reading.pdf",
        );
        assert_eq!(
            attachment_filename(
                None,
                "Notes",
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
            ),
            "Notes.docx",
        );
    }

    #[test]
    fn filename_keeps_an_extension_it_already_has() {
        assert_eq!(
            attachment_filename(Some("syllabus.pdf".into()), "Syllabus", Some("application/pdf")),
            "syllabus.pdf",
        );
    }

    #[test]
    fn server_suggested_filenames_are_sanitized() {
        // Zotero joins the filename onto its storage directory and rejects
        // anything that isn't a bare name.
        let out = attachment_filename(
            Some("readings/week 3.pdf".into()),
            "Week 3",
            Some("application/pdf"),
        );
        assert!(!out.contains('/'), "{out} still has a path separator");
    }

    #[test]
    fn unknown_content_type_leaves_the_name_alone() {
        assert_eq!(attachment_filename(None, "Mystery", Some("application/x-weird")), "Mystery");
        assert_eq!(attachment_filename(None, "Mystery", None), "Mystery");
    }
}
