use super::{merged_collections, ParentItem, ZoteroClient};
use crate::error::{AppError, Result};
use serde_json::{json, Map, Value};

impl ZoteroClient {
    /// Save a linked resource without downloading it or creating an attachment.
    /// Return whether the resource was created or its destination changed.
    pub async fn save_web_link(&self, item: &ParentItem, tag: &str) -> Result<(String, bool)> {
        let valid = item
            .url
            .as_deref()
            .and_then(|raw| url::Url::parse(raw).ok())
            .is_some_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.has_host()
                    && url.username().is_empty()
                    && url.password().is_none()
            });
        if !valid {
            return Err(AppError::BadRequest(
                "Linked resource must have a valid HTTP(S) URL".into(),
            ));
        }
        let Some(existing) = self.find_parent_by_brilliant_tag(tag).await? else {
            return self.create_parent_item(item).await.map(|key| (key, true));
        };
        let key = existing
            .pointer("/data/key")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::Other("Zotero linked resource is missing its key".into()))?;
        let changed = existing.pointer("/data/url").and_then(Value::as_str) != item.url.as_deref();
        // Preserve titles, notes, tags, and other metadata edited in Zotero.
        let mut patch = Map::new();
        if changed {
            patch.insert("url".into(), json!(item.url));
        }
        let mut collections = existing.pointer("/data/collections").cloned();
        let mut refiled = false;
        for collection in &item.collection_keys {
            if let Some(merged) = merged_collections(collections.as_ref(), collection) {
                collections = Some(json!(merged));
                refiled = true;
            }
        }
        if refiled {
            patch.insert("collections".into(), collections.unwrap());
        }
        if !patch.is_empty() {
            let version = existing
                .get("version")
                .or_else(|| existing.pointer("/data/version"))
                .and_then(Value::as_i64)
                .ok_or_else(|| {
                    AppError::Other("Zotero linked resource is missing its version".into())
                })?;
            let response = self
                .patch(&self.user_path(&format!("/items/{}", key)))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header("If-Unmodified-Since-Version", version.to_string())
                .json(&patch)
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(AppError::Other(format!(
                    "Zotero linked resource update: {}",
                    response.status()
                )));
            }
        }
        Ok((key.to_string(), changed))
    }
}
