//! Exercise the same linked-resource worker used by topic, module, and course sends.
use super::*;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::{sync::Mutex, task::JoinHandle};

#[derive(Default)]
struct Library {
    item: Mutex<Option<Value>>,
    calls: Mutex<Vec<(Method, String)>>,
    failure: AtomicUsize,
}

async fn request(
    State(library): State<Arc<Library>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    library
        .calls
        .lock()
        .await
        .push((method.clone(), uri.path().to_string()));
    if uri.path() == "/api/users/0/items" && method == Method::GET {
        let query = url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes());
        if let Some((_, tag)) = query.into_iter().find(|(key, _)| key == "tag") {
            if library.failure.load(Ordering::SeqCst) == 1 {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            if library.failure.load(Ordering::SeqCst) == 4 {
                return ([("content-type", "application/json")], "not JSON").into_response();
            }
            let item = library.item.lock().await;
            let found = item.as_ref().filter(|it| {
                it.pointer("/data/tags")
                    .and_then(Value::as_array)
                    .is_some_and(|tags| {
                        tags.iter()
                            .any(|t| t.get("tag").and_then(Value::as_str) == Some(tag.as_ref()))
                    })
            });
            return Json(found.into_iter().cloned().collect::<Vec<_>>()).into_response();
        }
        return Json(json!([])).into_response();
    }
    if uri.path() == "/api/users/0/items" && method == Method::POST {
        if library.failure.load(Ordering::SeqCst) == 2 {
            return StatusCode::FORBIDDEN.into_response();
        }
        let body: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["itemType"], "webpage");
        let mut data = body[0].clone();
        data["key"] = json!("LINK0001");
        data["version"] = json!(1);
        *library.item.lock().await = Some(json!({ "version": 1, "data": data }));
        return Json(json!({ "successful": { "0": { "key": "LINK0001" } } })).into_response();
    }
    if uri.path() == "/api/users/0/items/LINK0001" && method == Method::PATCH {
        if library.failure.load(Ordering::SeqCst) == 3 {
            return StatusCode::PRECONDITION_FAILED.into_response();
        }
        assert_eq!(headers.get("if-unmodified-since-version").unwrap(), "1");
        let patch: Value = serde_json::from_slice(&bytes).unwrap();
        let mut item = library.item.lock().await;
        for (key, value) in patch.as_object().unwrap() {
            assert!(
                key == "url" || key == "collections",
                "unexpected field {}",
                key
            );
            item.as_mut().unwrap()["data"][key] = value.clone();
        }
        return StatusCode::NO_CONTENT.into_response();
    }
    StatusCode::NOT_FOUND.into_response()
}

async fn fixture() -> (
    ZoteroClient,
    ZoteroMode,
    sqlx::SqlitePool,
    Arc<Library>,
    JoinHandle<()>,
) {
    let library = Arc::new(Library::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mode = ZoteroMode::Local {
        base_url: Some(format!("http://{}/api", listener.local_addr().unwrap())),
        basic_auth: None,
        user_id_override: None,
        api_key: None,
    };
    let app = Router::new()
        .route("/api/users/0/items", any(request))
        .route("/api/users/0/items/LINK0001", any(request))
        .with_state(library.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = ZoteroClient::new(mode.clone()).await.unwrap();
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/0025_zotero_document_links.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    (client, mode, pool, library, task)
}

async fn send(client: &ZoteroClient, pool: &sqlx::SqlitePool, url: &str) -> ZoteroResult {
    let mut target = CollectionTarget::new(client, "Biology".into());
    target.course_key = Some(Some("COURSE01".into()));
    target.enter_module("Week 1");
    target.module_key = Some(Some("MODULE01".into()));
    let mut result = ZoteroResult {
        created: vec![],
        failures: vec![],
        up_to_date: 0,
        collection_key: None,
    };
    send_link_to_zotero(
        client,
        pool,
        "course",
        "Biology",
        Some("BIO101"),
        &mut target,
        "topic",
        "Required reading",
        url.to_string(),
        &mut result,
    )
    .await;
    result
}

#[tokio::test]
async fn linked_resource_imports_metadata_into_the_module_and_records_its_zotero_destination() {
    let (client, mode, pool, library, task) = fixture().await;
    let result = send(&client, &pool, "https://example.test/reading").await;
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert_eq!(result.created, ["LINK0001"]);
    let item = library.item.lock().await.clone().unwrap();
    assert_eq!(item["data"]["url"], "https://example.test/reading");
    assert_eq!(item["data"]["collections"], json!(["MODULE01"]));
    assert_eq!(
        item["data"]["tags"],
        json!([{ "tag": "Brilliant: Biology" },
        { "tag": "brilliant:bsid=course:topic" }, { "tag": "BIO101" }])
    );
    assert_eq!(
        crate::zotero::links::load(&pool, &mode, "course")
            .await
            .unwrap()["topic"],
        "zotero://select/library/items/LINK0001"
    );
    assert!(library
        .calls
        .lock()
        .await
        .iter()
        .all(|(_, path)| !path.contains("/file") && !path.contains("/children")));
    task.abort();
}

#[tokio::test]
async fn resends_reuse_entries_update_changed_urls_and_preserve_user_metadata_and_collections() {
    let (client, _, pool, library, task) = fixture().await;
    send(&client, &pool, "https://example.test/reading").await;
    {
        let mut item = library.item.lock().await;
        let data = &mut item.as_mut().unwrap()["data"];
        data["title"] = json!("My edited title");
        data["abstractNote"] = json!("My notes");
        data["collections"] = json!(["MANUAL01"]);
    }
    let current = send(&client, &pool, "https://example.test/reading").await;
    assert_eq!(current.up_to_date, 1);
    assert!(current.created.is_empty());
    let changed = send(&client, &pool, "https://example.test/new-reading").await;
    assert_eq!(changed.created, ["LINK0001"]);
    let item = library.item.lock().await.clone().unwrap();
    assert_eq!(item["data"]["url"], "https://example.test/new-reading");
    assert_eq!(item["data"]["title"], "My edited title");
    assert_eq!(item["data"]["abstractNote"], "My notes");
    assert_eq!(item["data"]["collections"], json!(["MANUAL01", "MODULE01"]));
    assert_eq!(
        library
            .calls
            .lock()
            .await
            .iter()
            .filter(|(m, _)| *m == Method::POST)
            .count(),
        1
    );
    task.abort();
}

#[tokio::test]
async fn lookup_and_write_failures_do_not_record_links_or_create_duplicates() {
    let (client, mode, pool, library, task) = fixture().await;
    for failure in [1, 2, 4] {
        library.failure.store(failure, Ordering::SeqCst);
        let result = send(&client, &pool, "https://example.test/reading").await;
        assert_eq!(result.failures.len(), 1);
        assert!(result.created.is_empty());
        assert!(crate::zotero::links::load(&pool, &mode, "course")
            .await
            .unwrap()
            .is_empty());
        assert!(library.item.lock().await.is_none());
    }
    task.abort();
}

#[tokio::test]
async fn concurrent_changes_are_reported_without_overwriting_the_reference() {
    let (client, mode, pool, library, task) = fixture().await;
    send(&client, &pool, "https://example.test/reading").await;
    sqlx::query("DELETE FROM zotero_document_links")
        .execute(&pool)
        .await
        .unwrap();
    library.failure.store(3, Ordering::SeqCst);
    let result = send(&client, &pool, "https://example.test/new-reading").await;
    assert_eq!(result.failures.len(), 1);
    assert!(result.failures[0].contains("412"));
    assert!(crate::zotero::links::load(&pool, &mode, "course")
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        library.item.lock().await.as_ref().unwrap()["data"]["url"],
        "https://example.test/reading"
    );
    task.abort();
}
