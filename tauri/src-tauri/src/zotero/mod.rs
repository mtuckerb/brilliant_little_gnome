//! Thin Zotero Web API v3 client.
//!
//! Tucker uses local Zotero (desktop app's HTTP endpoint at
//! 127.0.0.1:23119/api), so that's the default. Cloud (api.zotero.org) is
//! opt-in via prefs and requires an API key + user ID. The local endpoint
//! mirrors the same Web API surface — same paths, same JSON shapes — so the
//! upload protocol works identically.
//!
//! Reference: https://www.zotero.org/support/dev/web_api/v3/start
//! Local endpoints: https://www.zotero.org/support/dev/local_http_endpoints
//! File upload protocol: https://www.zotero.org/support/dev/web_api/v3/file_upload
//!
//! The upload dance is genuinely multi-step:
//!   1. POST /items   — create the attachment item with `linkMode: "imported_file"`
//!   2. POST /items/<key>/file with md5/filename/filesize/mtime — get auth or `exists: 1`
//!   3. (unless exists) POST to the returned `url` with the file bytes wrapped in prefix/suffix
//!   4. POST /items/<key>/file with `upload=<key>` to finalize
//! `If-None-Match: *` is required on the file POSTs.

use crate::error::{AppError, Result};
use md5::{Digest, Md5};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const CLOUD_BASE: &str = "https://api.zotero.org";
const LOCAL_BASE: &str = "http://127.0.0.1:23119/api";
const LOCAL_USER_ID: &str = "0";
const ZOTERO_API_VERSION: &str = "3";

#[derive(Debug, Clone)]
pub enum ZoteroMode {
    /// Send to the desktop Zotero's local HTTP server (Preferences → Advanced
    /// → "Allow other applications on this computer to communicate with
    /// Zotero" must be enabled). No API key required for vanilla local
    /// usage. `base_url` overrides the default 127.0.0.1:23119/api — useful
    /// when reverse-proxying Zotero to a custom hostname.
    /// `basic_auth` covers the common reverse-proxy case where nginx (or
    /// similar) gates the Zotero endpoint behind HTTP Basic Auth. When set,
    /// the (username, password) pair is sent with every request.
    /// `user_id_override` lets the user supply a Zotero user ID other than
    /// the canonical local "0" — some reverse-proxied setups insist on
    /// the real cloud user ID even when the data lives locally.
    /// `api_key` is the Zotero API key, sent as `Zotero-API-Key`. Local
    /// HTTP Zotero ignores it; proxies that forward upstream to
    /// api.zotero.org need it.
    Local {
        base_url: Option<String>,
        basic_auth: Option<(String, String)>,
        user_id_override: Option<String>,
        api_key: Option<String>,
    },
    /// Send to api.zotero.org. Requires user ID + API key.
    Cloud { user_id: String, api_key: String },
}

pub struct ZoteroClient {
    http: Client,
    base: String,
    user_id: String,
    api_key: Option<String>,
    basic_auth: Option<(String, String)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Collection {
    pub key: String,
    pub data: CollectionData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CollectionData {
    pub name: String,
    #[serde(rename = "parentCollection", default, deserialize_with = "de_parent")]
    pub parent_collection: Option<String>,
}

/// What we need to know about an existing Zotero item to decide whether
/// to update it or skip. Returned by `find_by_brilliant_tag` so the
/// upper layer can compare MD5s without doing another round-trip.
#[derive(Debug, Clone)]
pub struct ExistingItem {
    pub key: String,
    /// MD5 of the first child attachment we find. None if the existing
    /// item has no file attachment yet (we'll replace by re-uploading).
    pub attachment_key: Option<String>,
    pub attachment_md5: Option<String>,
}

fn de_parent<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<String>, D::Error> {
    // Zotero returns `parentCollection: false` when the collection is at the
    // root, and a string key otherwise. Normalise to Option<String>.
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::String(s) => Some(s),
        _ => None,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ParentItem {
    pub item_type: ItemType,
    pub title: String,
    pub url: Option<String>,
    pub tags: Vec<String>,
    pub collection_keys: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum ItemType {
    Document,
    Webpage,
}

impl ItemType {
    fn as_str(self) -> &'static str {
        match self {
            ItemType::Document => "document",
            ItemType::Webpage => "webpage",
        }
    }
}

impl ZoteroClient {
    pub fn new(mode: ZoteroMode) -> Result<Self> {
        let http = Client::builder()
            .build()
            .map_err(|e| AppError::Other(format!("zotero client: {}", e)))?;
        let (base, user_id, api_key, basic_auth) = match mode {
            ZoteroMode::Local { base_url, basic_auth, user_id_override, api_key } => {
                let base = base_url
                    .as_ref()
                    .map(|s| s.trim().trim_end_matches('/').to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| LOCAL_BASE.to_string());
                let creds = basic_auth.and_then(|(u, p)| {
                    let u = u.trim().to_string();
                    let p = p.trim().to_string();
                    // Only send Basic Auth when both halves are filled in;
                    // a half-blank credential just produces a 401 anyway.
                    if u.is_empty() || p.is_empty() { None } else { Some((u, p)) }
                });
                let user_id = user_id_override
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| LOCAL_USER_ID.to_string());
                let key = api_key
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                (base, user_id, key, creds)
            }
            ZoteroMode::Cloud { user_id, api_key } => {
                let uid = user_id.trim().to_string();
                let key = api_key.trim().to_string();
                if uid.is_empty() || key.is_empty() {
                    return Err(AppError::Other(
                        "Cloud Zotero requires a user ID and API key (Settings → Zotero)".into(),
                    ));
                }
                (CLOUD_BASE.to_string(), uid, Some(key), None)
            }
        };
        tracing::info!(
            "zotero client: base={} user_id={} basic_auth={}",
            base,
            user_id,
            if basic_auth.is_some() { "yes" } else { "no" },
        );
        Ok(Self { http, base, user_id, api_key, basic_auth })
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.basic_auth {
            Some((u, p)) => req.basic_auth(u, Some(p)),
            None => req,
        }
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.apply_auth(self.http.get(url).headers(self.auth_headers()))
    }

    fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.apply_auth(self.http.post(url).headers(self.auth_headers()))
    }

    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("Zotero-API-Version", ZOTERO_API_VERSION.parse().unwrap());
        // Local endpoint accepts (and ignores) requests with no API key;
        // omitting the header keeps things clean.
        if let Some(key) = self.api_key.as_deref() {
            h.insert("Zotero-API-Key", key.parse().unwrap());
        }
        h
    }

    fn user_path(&self, suffix: &str) -> String {
        format!("{}/users/{}{}", self.base, self.user_id, suffix)
    }

    /// List every collection in the user's library. Pages are returned in
    /// chunks of up to 100; we walk until we've seen them all.
    pub async fn list_collections(&self) -> Result<Vec<Collection>> {
        let mut out = Vec::new();
        let mut start = 0usize;
        loop {
            let url = self.user_path("/collections");
            let resp = self
                .get(&url)
                .query(&[("start", start.to_string()), ("limit", "100".to_string())])
                .send()
                .await
                .map_err(|e| AppError::Other(format!("zotero list_collections: {}", e)))?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(AppError::Other(format!("zotero list_collections {}: {}", status, body)));
            }
            let page: Vec<Collection> = resp
                .json()
                .await
                .map_err(|e| AppError::Other(format!("zotero collections parse: {}", e)))?;
            let n = page.len();
            out.extend(page);
            if n < 100 {
                break;
            }
            start += n;
        }
        Ok(out)
    }

    /// Get-or-create a collection by name. `parent` is the *preferred*
    /// parent for new creates, but lookup matches by name globally — if
    /// the user has reorganized their Zotero library and moved this
    /// collection to a different parent, we honor the move and reuse the
    /// existing key rather than creating a duplicate at the original
    /// location. Exact (name + parent) matches are preferred when
    /// multiple collections share the same name.
    pub async fn ensure_collection(&self, name: &str, parent: Option<&str>) -> Result<String> {
        let existing = self.list_collections().await?;
        // Prefer exact (name + parent) match.
        if let Some(c) = existing.iter().find(|c| {
            c.data.name == name && c.data.parent_collection.as_deref() == parent
        }) {
            return Ok(c.key.clone());
        }
        // Fall back to any collection with this name — user moved it.
        if let Some(c) = existing.iter().find(|c| c.data.name == name) {
            tracing::info!(
                "zotero collection '{}' reused at parent={:?} (requested parent={:?})",
                name,
                c.data.parent_collection,
                parent
            );
            return Ok(c.key.clone());
        }
        let body = json!([{
            "name": name,
            "parentCollection": parent.map(Value::from).unwrap_or(Value::Bool(false)),
        }]);
        let url = self.user_path("/collections");
        let resp = self
            .post(&url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero create collection: {}", e)))?;
        let status = resp.status();
        let payload: Value = resp
            .json()
            .await
            .map_err(|e| AppError::Other(format!("zotero create collection parse: {}", e)))?;
        if !status.is_success() {
            return Err(AppError::Other(format!("zotero create collection {}: {}", status, payload)));
        }
        let key = payload
            .pointer("/successful/0/key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Other(format!("zotero create collection — no key in {}", payload)))?
            .to_string();
        Ok(key)
    }

    /// Look up an item we previously created by its Brilliant tag (e.g.
    /// `brilliant:bsid=<topic_id>`). Returns the parent item's key plus
    /// — when present — its first file attachment's key and MD5 so the
    /// caller can decide replace vs. skip. None when no item is tagged.
    pub async fn find_by_brilliant_tag(&self, tag: &str) -> Result<Option<ExistingItem>> {
        let url = self.user_path("/items");
        let resp = self
            .get(&url)
            .query(&[("tag", tag), ("limit", "5")])
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero tag search: {}", e)))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::Other(format!("zotero tag search read: {}", e)))?;
        if !status.is_success() {
            return Err(AppError::Other(format!(
                "zotero tag search {}: {}",
                status,
                truncate_for_error(&body)
            )));
        }
        let arr: Vec<Value> = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => return Ok(None), // unparseable → treat as "not found"
        };
        // Prefer a parent item (one without a parentItem field). Only fall
        // back to a child attachment match if there's nothing else.
        let parent = arr.iter().find(|it| {
            it.pointer("/data/parentItem").is_none() && it.pointer("/data/key").is_some()
        });
        let parent_key = match parent.and_then(|it| it.pointer("/data/key").and_then(|v| v.as_str())) {
            Some(k) => k.to_string(),
            None => return Ok(None),
        };

        // Walk the parent's children to find the file attachment.
        let children_url = self.user_path(&format!("/items/{}/children", parent_key));
        let cresp = self
            .get(&children_url)
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero children fetch: {}", e)))?;
        let cstatus = cresp.status();
        let cbody = cresp
            .text()
            .await
            .map_err(|e| AppError::Other(format!("zotero children read: {}", e)))?;
        if !cstatus.is_success() {
            return Err(AppError::Other(format!(
                "zotero children {}: {}",
                cstatus,
                truncate_for_error(&cbody)
            )));
        }
        let children: Vec<Value> = serde_json::from_str(&cbody).unwrap_or_default();
        let attach = children.iter().find(|it| {
            it.pointer("/data/itemType")
                .and_then(|v| v.as_str())
                .map(|s| s == "attachment")
                .unwrap_or(false)
        });
        let attachment_key = attach
            .and_then(|it| it.pointer("/data/key").and_then(|v| v.as_str()))
            .map(String::from);
        let attachment_md5 = attach
            .and_then(|it| it.pointer("/data/md5").and_then(|v| v.as_str()))
            .map(String::from);
        Ok(Some(ExistingItem { key: parent_key, attachment_key, attachment_md5 }))
    }

    /// Replace the file behind an existing attachment item. Same upload
    /// protocol as a new attachment but POSTs against the existing key
    /// instead of creating a fresh attachment-item shell first.
    pub async fn replace_attachment_file(
        &self,
        attachment_key: &str,
        filename: &str,
        content_type: Option<&str>,
        bytes: &[u8],
    ) -> Result<()> {
        let md5 = format!("{:x}", Md5::digest(bytes));
        let mtime = chrono::Utc::now().timestamp_millis();
        let auth_url = self.user_path(&format!("/items/{}/file", attachment_key));
        let auth_form = [
            ("md5", md5.as_str()),
            ("filename", filename),
            ("filesize", &bytes.len().to_string()),
            ("mtime", &mtime.to_string()),
        ];
        // `If-Match: <existing_md5>` is more correct here but we'd need
        // an extra round-trip to fetch it. The `*` form replaces unconditionally
        // which is what the caller has already decided to do.
        let auth_resp = self
            .post(&auth_url)
            .header("If-Match", "*")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&auth_form)
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero replace auth: {}", e)))?;
        let auth_status = auth_resp.status();
        let auth_body = auth_resp
            .text()
            .await
            .map_err(|e| AppError::Other(format!("zotero replace auth read: {}", e)))?;
        if !auth_status.is_success() {
            return Err(AppError::Other(format!(
                "zotero replace auth {}: {}",
                auth_status,
                truncate_for_error(&auth_body)
            )));
        }
        let auth_payload: Value = serde_json::from_str(&auth_body).map_err(|e| {
            AppError::Other(format!(
                "zotero replace auth parse ({}): body: {}",
                e,
                truncate_for_error(&auth_body)
            ))
        })?;
        if auth_payload.get("exists").and_then(|v| v.as_i64()).unwrap_or(0) == 1 {
            return Ok(());
        }
        let upload_url = auth_payload
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Other("zotero replace: missing upload url".into()))?
            .to_string();
        let prefix = auth_payload.get("prefix").and_then(|v| v.as_str()).unwrap_or("");
        let suffix = auth_payload.get("suffix").and_then(|v| v.as_str()).unwrap_or("");
        let upload_content_type = auth_payload
            .get("contentType")
            .and_then(|v| v.as_str())
            .unwrap_or("multipart/form-data");
        let mut wrapped = Vec::with_capacity(prefix.len() + bytes.len() + suffix.len());
        wrapped.extend_from_slice(prefix.as_bytes());
        wrapped.extend_from_slice(bytes);
        wrapped.extend_from_slice(suffix.as_bytes());
        let s3 = self
            .http
            .post(&upload_url)
            .header(header::CONTENT_TYPE, upload_content_type)
            .body(wrapped)
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero replace s3: {}", e)))?;
        if !s3.status().is_success() {
            let st = s3.status();
            let bd = s3.text().await.unwrap_or_default();
            return Err(AppError::Other(format!("zotero replace s3 {}: {}", st, bd)));
        }
        let upload_key = auth_payload
            .get("uploadKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Other("zotero replace: missing uploadKey".into()))?
            .to_string();
        let register_form = [("upload", upload_key.as_str())];
        let register = self
            .post(&auth_url)
            .header("If-Match", "*")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&register_form)
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero replace register: {}", e)))?;
        if !register.status().is_success() {
            let st = register.status();
            let bd = register.text().await.unwrap_or_default();
            return Err(AppError::Other(format!("zotero replace register {}: {}", st, bd)));
        }
        // Also bump filename/contentType so a renamed source file is
        // reflected in the attachment metadata.
        if let Some(ct) = content_type {
            let url = self.user_path(&format!("/items/{}", attachment_key));
            let _ = self
                .http
                .patch(&url)
                .headers(self.auth_headers())
                .header(header::CONTENT_TYPE, "application/json")
                .body(json!({ "filename": filename, "contentType": ct }).to_string())
                .send()
                .await;
        }
        Ok(())
    }

    /// Compute the MD5 of a byte slice as the lowercase hex string Zotero
    /// uses on attachment metadata. Exposed so the upper layer can compare
    /// against an existing attachment without re-implementing the digest.
    pub fn md5_of(bytes: &[u8]) -> String {
        format!("{:x}", Md5::digest(bytes))
    }

    /// Create a parent item (document/webpage style). Returns its key.
    pub async fn create_parent_item(&self, item: &ParentItem) -> Result<String> {
        let mut body = json!({
            "itemType": item.item_type.as_str(),
            "title": item.title,
            "collections": item.collection_keys,
            "tags": item.tags.iter().map(|t| json!({ "tag": t })).collect::<Vec<_>>(),
        });
        if let Some(url) = item.url.as_ref() {
            body["url"] = Value::String(url.clone());
        }
        let payload = json!([body]);
        let url = self.user_path("/items");
        let resp = self
            .post(&url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero create item: {}", e)))?;
        let status = resp.status();
        // Capture body as text first — when a reverse proxy returns an
        // HTML error page or the server replies in an unexpected shape,
        // we still get to see what it actually said instead of a generic
        // "error decoding response body".
        let body_text = resp
            .text()
            .await
            .map_err(|e| AppError::Other(format!("zotero create item read: {}", e)))?;
        if !status.is_success() {
            return Err(AppError::Other(format!(
                "zotero create item {}: {}",
                status,
                truncate_for_error(&body_text)
            )));
        }
        let payload: Value = serde_json::from_str(&body_text).map_err(|e| {
            AppError::Other(format!(
                "zotero create item parse ({}): body was: {}",
                e,
                truncate_for_error(&body_text)
            ))
        })?;
        let key = payload
            .pointer("/successful/0/key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Other(format!(
                    "zotero create item — no key in response: {}",
                    truncate_for_error(&body_text)
                ))
            })?
            .to_string();
        Ok(key)
    }

    /// Create an imported-file attachment as a child of `parent_key`, then
    /// upload the bytes. Returns the attachment item's key.
    pub async fn upload_attachment(
        &self,
        parent_key: &str,
        filename: &str,
        content_type: Option<&str>,
        bytes: &[u8],
    ) -> Result<String> {
        // Step 1: create the attachment item.
        let attachment_body = json!([{
            "itemType": "attachment",
            "linkMode": "imported_file",
            "title": filename,
            "filename": filename,
            "contentType": content_type.unwrap_or("application/octet-stream"),
            "parentItem": parent_key,
            "tags": [],
            "collections": [],
        }]);
        let url = self.user_path("/items");
        let resp = self
            .post(&url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(attachment_body.to_string())
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero attach create: {}", e)))?;
        let status = resp.status();
        let payload: Value = resp
            .json()
            .await
            .map_err(|e| AppError::Other(format!("zotero attach create parse: {}", e)))?;
        if !status.is_success() {
            return Err(AppError::Other(format!("zotero attach create {}: {}", status, payload)));
        }
        let attachment_key = payload
            .pointer("/successful/0/key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Other(format!("zotero attach — no key in {}", payload)))?
            .to_string();

        // Step 2: request upload authorization.
        let md5 = format!("{:x}", Md5::digest(bytes));
        let mtime = (chrono::Utc::now().timestamp_millis()) as i64;
        let auth_url = self.user_path(&format!("/items/{}/file", attachment_key));
        let auth_form = [
            ("md5", md5.as_str()),
            ("filename", filename),
            ("filesize", &bytes.len().to_string()),
            ("mtime", &mtime.to_string()),
        ];
        let auth_resp = self
            .post(&auth_url)
            .header("If-None-Match", "*")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&auth_form)
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero upload auth: {}", e)))?;
        let auth_status = auth_resp.status();
        let auth_payload: Value = auth_resp
            .json()
            .await
            .map_err(|e| AppError::Other(format!("zotero upload auth parse: {}", e)))?;
        if !auth_status.is_success() {
            return Err(AppError::Other(format!("zotero upload auth {}: {}", auth_status, auth_payload)));
        }
        if auth_payload.get("exists").and_then(|v| v.as_i64()).unwrap_or(0) == 1 {
            // Identical file already on Zotero — nothing else to do.
            return Ok(attachment_key);
        }

        // Step 3: upload the wrapped bytes to S3.
        let upload_url = auth_payload
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Other(format!("zotero upload auth missing url: {}", auth_payload)))?
            .to_string();
        let prefix = auth_payload.get("prefix").and_then(|v| v.as_str()).unwrap_or("");
        let suffix = auth_payload.get("suffix").and_then(|v| v.as_str()).unwrap_or("");
        let upload_content_type = auth_payload
            .get("contentType")
            .and_then(|v| v.as_str())
            .unwrap_or("multipart/form-data");
        let mut wrapped = Vec::with_capacity(prefix.len() + bytes.len() + suffix.len());
        wrapped.extend_from_slice(prefix.as_bytes());
        wrapped.extend_from_slice(bytes);
        wrapped.extend_from_slice(suffix.as_bytes());
        let s3_resp = self
            .http
            .post(&upload_url)
            .header(header::CONTENT_TYPE, upload_content_type)
            .body(wrapped)
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero s3 upload: {}", e)))?;
        if !s3_resp.status().is_success() {
            let status = s3_resp.status();
            let body = s3_resp.text().await.unwrap_or_default();
            return Err(AppError::Other(format!("zotero s3 upload {}: {}", status, body)));
        }

        // Step 4: register the upload with Zotero.
        let upload_key = auth_payload
            .get("uploadKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Other(format!("zotero upload auth missing uploadKey: {}", auth_payload)))?
            .to_string();
        let register_form = [("upload", upload_key.as_str())];
        let register_resp = self
            .post(&auth_url)
            .header("If-None-Match", "*")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&register_form)
            .send()
            .await
            .map_err(|e| AppError::Other(format!("zotero upload register: {}", e)))?;
        if !register_resp.status().is_success() {
            let status = register_resp.status();
            let body = register_resp.text().await.unwrap_or_default();
            return Err(AppError::Other(format!("zotero upload register {}: {}", status, body)));
        }

        Ok(attachment_key)
    }
}

/// Trim a server response body so it fits in a toast / log line without
/// dragging the whole HTML page through the UI. Keeps the first 300
/// chars; everything else is replaced with "…".
fn truncate_for_error(s: &str) -> String {
    const MAX: usize = 300;
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        let head: String = s.chars().take(MAX).collect();
        format!("{}…", head)
    }
}
