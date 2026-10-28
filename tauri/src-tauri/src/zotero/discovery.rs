//! Read-only recovery of destinations for courses sent before links were saved.
//! One paginated metadata scan recovers every course in the configured library.

use super::{ZoteroClient, ZoteroMode};
use crate::error::{AppError, Result};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct BackfillCoordinator {
    attempts: Mutex<HashMap<String, Attempt>>,
}

enum Attempt {
    Running,
    Complete,
    RetryAfter(Instant),
}

impl BackfillCoordinator {
    /// Coalesce launch, settings, and page-load requests without waiting on HTTP.
    pub fn request(
        self: &Arc<Self>,
        pool: SqlitePool,
        mode: ZoteroMode,
        on_updated: impl FnOnce() + Send + 'static,
    ) {
        let library = mode.library_id();
        {
            let mut attempts = self.attempts.lock().unwrap();
            match attempts.get(&library) {
                Some(Attempt::Running | Attempt::Complete) => return,
                Some(Attempt::RetryAfter(at)) if *at > Instant::now() => return,
                _ => {}
            }
            attempts.insert(library.clone(), Attempt::Running);
        }
        let coordinator = self.clone();
        tokio::spawn(async move {
            let result =
                tokio::time::timeout(Duration::from_secs(180), backfill(&pool, mode)).await;
            let next = match result {
                Ok(Ok(updated)) => {
                    if updated > 0 {
                        on_updated();
                    }
                    Attempt::Complete
                }
                error => {
                    tracing::debug!("Zotero link discovery will retry: {:?}", error);
                    Attempt::RetryAfter(Instant::now() + Duration::from_secs(60))
                }
            };
            coordinator.attempts.lock().unwrap().insert(library, next);
        });
    }
}

fn checkpoint_key(mode: &ZoteroMode) -> String {
    format!("zotero.document-links-backfilled.v2:{}", mode.library_id())
}

async fn backfill(pool: &SqlitePool, mode: ZoteroMode) -> Result<u64> {
    let checkpoint = checkpoint_key(&mode);
    let done: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM app_state WHERE key = ?)")
        .bind(&checkpoint)
        .fetch_one(pool)
        .await?;
    if done {
        return Ok(0);
    }
    let client = ZoteroClient::new(mode).await?;
    let items = client.existing_library_items().await?;
    let documents = documents(&items);
    let mut tx = pool.begin().await?;
    let mut updated = 0;
    for document in documents {
        // A send may have completed during discovery. Its saved destination wins.
        updated += sqlx::query(
            "INSERT INTO zotero_document_links (library_id, course_id, source_id, url)
             VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(client.mode.library_id())
        .bind(document.course_id)
        .bind(document.source_id)
        .bind(
            client
                .mode
                .document_url(&document.attachment_key, document.mime.as_deref()),
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }
    // Failed, interrupted, or incomplete scans never mark a library complete.
    sqlx::query("INSERT INTO app_state (key, value) VALUES (?, '1') ON CONFLICT(key) DO UPDATE SET value = '1'")
        .bind(checkpoint).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(updated)
}

impl ZoteroClient {
    async fn existing_library_items(&self) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        let mut start = 0;
        let mut library_version = None;
        loop {
            let response = self
                .get(&self.user_path("/items"))
                .query(&[
                    ("limit", "100"),
                    ("start", &start.to_string()),
                    ("format", "json"),
                ])
                .timeout(Duration::from_secs(20))
                .send()
                .await?;
            let status = response.status();
            if !status.is_success() {
                return Err(AppError::Other(format!(
                    "Zotero link discovery: {}",
                    status
                )));
            }
            if let Some(version) = response.headers().get("Last-Modified-Version") {
                if library_version
                    .as_ref()
                    .is_some_and(|previous| previous != version)
                {
                    return Err(AppError::Other(
                        "Zotero library changed during link discovery".into(),
                    ));
                }
                library_version = Some(version.clone());
            }
            let total = response
                .headers()
                .get("Total-Results")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<usize>().ok());
            let backoff = response
                .headers()
                .get("Backoff")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            let page: Vec<Value> = response.json().await?;
            let count = page.len();
            items.extend(page);
            if count < 100 {
                if total.is_some_and(|total| items.len() < total) {
                    return Err(AppError::Other(
                        "Zotero returned an incomplete library page".into(),
                    ));
                }
                return Ok(items);
            }
            start += count;
            if let Some(seconds) = backoff {
                tokio::time::sleep(Duration::from_secs(seconds)).await;
            }
        }
    }
}

struct Document {
    course_id: String,
    source_id: String,
    attachment_key: String,
    mime: Option<String>,
}

fn deleted(data: &Value) -> bool {
    data.get("deleted").is_some_and(|v| v == true || v == 1)
}

fn item_key(data: &Value) -> Option<&str> {
    data.get("key").and_then(Value::as_str).filter(|key| {
        key.len() == 8
            && key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    })
}

fn documents(items: &[Value]) -> Vec<Document> {
    let parents: HashMap<_, _> = items
        .iter()
        .filter_map(|item| {
            let data = item.get("data")?;
            if deleted(data)
                || data.get("parentItem").and_then(Value::as_str).is_some()
                || data.get("itemType").and_then(Value::as_str) == Some("attachment")
            {
                return None;
            }
            Some((item_key(data)?, data))
        })
        .collect();
    let mut found: HashMap<(String, String), Document> = HashMap::new();
    for item in items {
        let Some(data) = item.get("data") else {
            continue;
        };
        if deleted(data) || data.get("itemType").and_then(Value::as_str) != Some("attachment") {
            continue;
        }
        // A URL-only attachment or an unfinished upload is not a document.
        let mode = data.get("linkMode").and_then(Value::as_str);
        if !matches!(mode, Some("imported_file" | "imported_url"))
            || data
                .get("md5")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .is_none()
        {
            continue;
        }
        let Some(key) = item_key(data) else { continue };
        let Some(parent) = data
            .get("parentItem")
            .and_then(Value::as_str)
            .and_then(|key| parents.get(key))
        else {
            continue;
        };
        for (course, source) in sources(parent) {
            let mime = data
                .get("contentType")
                .and_then(Value::as_str)
                .map(String::from)
                .or_else(|| {
                    data.get("filename")
                        .and_then(Value::as_str)
                        .filter(|name| name.to_ascii_lowercase().ends_with(".pdf"))
                        .map(|_| "application/pdf".to_string())
                });
            let document = Document {
                course_id: course.to_string(),
                source_id: source.to_string(),
                attachment_key: key.to_string(),
                mime,
            };
            let id = (course.to_string(), source.to_string());
            // Prefer a PDF if the user has since attached additional file formats.
            if !found.contains_key(&id) || document.mime.as_deref() == Some("application/pdf") {
                found.insert(id, document);
            }
        }
    }
    // A saved webpage needs no child attachment. Prefer a confirmed file above
    // if the user subsequently attached one to the same reference.
    for (key, parent) in parents {
        if parent.get("itemType").and_then(Value::as_str) != Some("webpage")
            || !parent
                .get("url")
                .and_then(Value::as_str)
                .and_then(|raw| url::Url::parse(raw).ok())
                .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.has_host())
        {
            continue;
        }
        for (course, source) in sources(parent) {
            found
                .entry((course.to_string(), source.to_string()))
                .or_insert_with(|| Document {
                    course_id: course.to_string(),
                    source_id: source.to_string(),
                    attachment_key: key.to_string(),
                    mime: None,
                });
        }
    }
    found.into_values().collect()
}

fn sources(parent: &Value) -> Vec<(&str, &str)> {
    parent
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tag| tag.get("tag").and_then(Value::as_str))
        .filter_map(|tag| {
            tag.strip_prefix("brilliant:bsid=")
                .and_then(|s| s.split_once(':'))
                .or_else(|| {
                    tag.strip_prefix("brilliant:syllabus=")
                        .map(|course| (course, "syllabus"))
                })
        })
        .filter(|(course, source)| !course.is_empty() && !source.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zotero::links;
    use axum::{
        extract::{Query, State},
        http::{Method, StatusCode},
        response::{IntoResponse, Response},
        routing::any,
        Json, Router,
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::task::JoinHandle;

    struct Fixture {
        items: Vec<Value>,
        requests: Mutex<Vec<Method>>,
        failure: AtomicUsize,
    }

    async fn metadata(
        State(fixture): State<Arc<Fixture>>,
        Query(query): Query<HashMap<String, String>>,
        method: Method,
    ) -> Response {
        fixture.requests.lock().unwrap().push(method.clone());
        if method != Method::GET {
            return StatusCode::METHOD_NOT_ALLOWED.into_response();
        }
        if query.get("limit").map(String::as_str) == Some("1") {
            return Json(json!([])).into_response(); // custom-server base probe
        }
        let start = query.get("start").unwrap().parse::<usize>().unwrap();
        if start >= 100 {
            match fixture.failure.load(Ordering::SeqCst) {
                1 => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
                2 => return ([("content-type", "application/json")], "not JSON").into_response(),
                _ => {}
            }
        }
        let mut response = Json(
            fixture
                .items
                .iter()
                .skip(start)
                .take(100)
                .cloned()
                .collect::<Vec<_>>(),
        )
        .into_response();
        response.headers_mut().insert(
            "Last-Modified-Version",
            if start >= 100 && fixture.failure.load(Ordering::SeqCst) == 3 {
                "2"
            } else {
                "1"
            }
            .parse()
            .unwrap(),
        );
        response.headers_mut().insert(
            "Total-Results",
            fixture.items.len().to_string().parse().unwrap(),
        );
        response
    }

    fn parent(key: &str, tag: &str) -> Value {
        json!({ "data": { "key": key, "itemType": "document", "tags": [{ "tag": tag }] } })
    }

    fn attachment(key: &str, parent: &str) -> Value {
        json!({ "data": { "key": key, "itemType": "attachment", "parentItem": parent,
            "linkMode": "imported_file", "md5": "confirmed-file-hash", "contentType": "application/pdf" } })
    }

    fn old_library() -> Vec<Value> {
        let mut items = vec![
            parent("PARENT01", "brilliant:bsid=1:11"),
            parent("PARENT02", "brilliant:bsid=2:22"),
            parent("PARENT03", "brilliant:syllabus=1"),
        ];
        items.extend((0..97).map(|_| json!({ "data": { "itemType": "note" } })));
        // Children can land on a different page from their tagged parents.
        items.extend([
            attachment("ATTACH01", "PARENT01"),
            attachment("ATTACH02", "PARENT02"),
            attachment("ATTACH03", "PARENT03"),
        ]);
        items
    }

    async fn server() -> (ZoteroMode, Arc<Fixture>, JoinHandle<()>) {
        let fixture = Arc::new(Fixture {
            items: old_library(),
            requests: Mutex::default(),
            failure: AtomicUsize::new(0),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/api/users/0/items", any(metadata))
            .with_state(fixture.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (
            ZoteroMode::Local {
                base_url: Some(base),
                user_id_override: None,
                api_key: None,
                basic_auth: None,
            },
            fixture,
            task,
        )
    }

    async fn database() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0002_app_state.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/0025_zotero_document_links.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn recovers_previous_courses_and_syllabus_without_uploading_or_overwriting_links() {
        let (mode, fixture, server) = server().await;
        let pool = database().await;
        // Upgrading from file-only recovery must also discover webpage entries.
        sqlx::query("INSERT INTO app_state (key, value) VALUES (?, '1')")
            .bind(format!(
                "zotero.document-links-backfilled.v1:{}",
                mode.library_id()
            ))
            .execute(&pool)
            .await
            .unwrap();
        let client = ZoteroClient::new(mode.clone()).await.unwrap();
        client
            .remember_document(&pool, "1", "11", "NEWFILE1", Some("application/pdf"))
            .await
            .unwrap();
        assert_eq!(backfill(&pool, mode.clone()).await.unwrap(), 2);
        let first = links::load(&pool, &mode, "1").await.unwrap();
        assert_eq!(first["11"], "zotero://open-pdf/library/items/NEWFILE1");
        assert_eq!(
            first["syllabus"],
            "zotero://open-pdf/library/items/ATTACH03"
        );
        assert_eq!(
            links::load(&pool, &mode, "2").await.unwrap()["22"],
            "zotero://open-pdf/library/items/ATTACH02"
        );
        let requests = fixture.requests.lock().unwrap().len();
        assert_eq!(backfill(&pool, mode).await.unwrap(), 0);
        assert_eq!(
            fixture.requests.lock().unwrap().len(),
            requests,
            "checkpoint survives restarting the coordinator"
        );
        assert!(fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|m| *m == Method::GET));
        server.abort();
    }

    #[tokio::test]
    async fn incomplete_or_malformed_scans_are_not_checkpointed_and_can_retry() {
        let (mode, fixture, server) = server().await;
        let pool = database().await;
        for failure in [1, 2, 3] {
            fixture.failure.store(failure, Ordering::SeqCst);
            assert!(backfill(&pool, mode.clone()).await.is_err());
            assert!(links::load(&pool, &mode, "1").await.unwrap().is_empty());
            let checkpoints: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_state")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(checkpoints, 0);
        }
        fixture.failure.store(0, Ordering::SeqCst);
        assert_eq!(backfill(&pool, mode.clone()).await.unwrap(), 3);
        assert_eq!(links::load(&pool, &mode, "1").await.unwrap().len(), 2);
        server.abort();
    }

    #[tokio::test]
    async fn simultaneous_requests_coalesce_and_retry_after_zotero_recovers() {
        let (mode, fixture, server) = server().await;
        let pool = database().await;
        fixture.failure.store(1, Ordering::SeqCst);
        let coordinator = Arc::new(BackfillCoordinator::default());
        let notifications = Arc::new(AtomicUsize::new(0));
        for _ in 0..4 {
            let notifications = notifications.clone();
            coordinator.request(pool.clone(), mode.clone(), move || {
                notifications.fetch_add(1, Ordering::SeqCst);
            });
        }
        wait_for(&coordinator, &mode, false).await;
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture.requests.lock().unwrap().len(),
            3,
            "one probe and two pages, without duplicate scans"
        );
        fixture.failure.store(0, Ordering::SeqCst);
        coordinator
            .attempts
            .lock()
            .unwrap()
            .insert(mode.library_id(), Attempt::RetryAfter(Instant::now()));
        let updated = notifications.clone();
        coordinator.request(pool.clone(), mode.clone(), move || {
            updated.fetch_add(1, Ordering::SeqCst);
        });
        wait_for(&coordinator, &mode, true).await;
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        assert_eq!(links::load(&pool, &mode, "1").await.unwrap().len(), 2);
        server.abort();
    }

    async fn wait_for(coordinator: &BackfillCoordinator, mode: &ZoteroMode, complete: bool) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let finished = match coordinator.attempts.lock().unwrap().get(&mode.library_id()) {
                    Some(Attempt::Complete) => complete,
                    Some(Attempt::RetryAfter(_)) => !complete,
                    _ => false,
                };
                if finished {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }

    #[test]
    fn skips_deleted_url_only_unfinished_and_untagged_items() {
        let mut items = vec![parent("PARENT01", "brilliant:bsid=1:11")];
        let mut deleted_parent = parent("PARENT02", "brilliant:bsid=2:22");
        deleted_parent["data"]["deleted"] = json!(1);
        items.extend([deleted_parent, parent("PARENT03", "an unrelated tag")]);
        let mut unfinished = attachment("ATTACH01", "PARENT01");
        unfinished["data"]["md5"] = Value::Null;
        let mut url_only = attachment("ATTACH02", "PARENT01");
        url_only["data"]["linkMode"] = json!("linked_url");
        let mut deleted = attachment("ATTACH03", "PARENT01");
        deleted["data"]["deleted"] = json!(true);
        items.extend([
            unfinished,
            url_only,
            deleted,
            attachment("ATTACH04", "PARENT02"),
            attachment("ATTACH05", "PARENT03"),
        ]);
        assert!(documents(&items).is_empty());
        items.push(attachment("ATTACH06", "PARENT01"));
        let found = documents(&items);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].attachment_key, "ATTACH06");
    }

    #[test]
    fn recovers_tagged_webpages_without_attachments_and_prefers_a_file_if_attached_later() {
        let mut webpage = parent("PARENT01", "brilliant:bsid=1:11");
        webpage["data"]["itemType"] = json!("webpage");
        webpage["data"]["url"] = json!("https://example.test/reading");
        let mut items = vec![webpage];
        let recovered = documents(&items);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].attachment_key, "PARENT01");
        assert!(recovered[0].mime.is_none());
        items.push(attachment("ATTACH01", "PARENT01"));
        assert_eq!(documents(&items)[0].attachment_key, "ATTACH01");
        items[0]["data"]["url"] = json!("javascript:alert(1)");
        items.pop();
        assert!(documents(&items).is_empty());
    }
}
