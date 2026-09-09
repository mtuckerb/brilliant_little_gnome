use super::*;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Form, Json, Router,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

const OLD_MD5: &str = "149603e6c03516362a8da23f624db945";
const NEW_BYTES: &[u8] = b"updated syllabus";

#[derive(Clone)]
struct UploadServer {
    base: String,
    expected_md5: Option<String>,
    conflict_on: Option<&'static str>,
    exists: bool,
    calls: Arc<Mutex<Vec<String>>>,
}

async fn file_request(
    State(state): State<UploadServer>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let stage = if form.contains_key("upload") {
        "register"
    } else {
        "authorize"
    };
    state.calls.lock().await.push(stage.into());
    // Match Zotero's file preconditions. A wildcard If-Match is invalid;
    // the old file hash is required at BOTH stages, never the new hash.
    let matches = match state.expected_md5.as_deref() {
        Some(md5) => {
            headers.get("if-match").and_then(|v| v.to_str().ok()) == Some(md5)
                && !headers.contains_key("if-none-match")
        }
        None => {
            headers.get("if-none-match").and_then(|v| v.to_str().ok()) == Some("*")
                && !headers.contains_key("if-match")
        }
    };
    if !matches {
        return (StatusCode::BAD_REQUEST, "Invalid ETag in If-Match header").into_response();
    }
    if state.conflict_on == Some(stage) {
        return (
            StatusCode::PRECONDITION_FAILED,
            "ETag does not match current version of file",
        )
            .into_response();
    }
    if stage == "register" {
        assert_eq!(
            form.get("upload").map(String::as_str),
            Some("fixture-upload")
        );
        return StatusCode::NO_CONTENT.into_response();
    }
    assert_eq!(form.get("md5"), Some(&ZoteroClient::md5_of(NEW_BYTES)));
    if state.exists {
        return Json(json!({"exists": 1})).into_response();
    }
    Json(json!({
        "url": format!("{}/upload", state.base),
        "prefix": "begin:", "suffix": ":end", "contentType": "application/octet-stream",
        "uploadKey": "fixture-upload"
    }))
    .into_response()
}

async fn upload(State(state): State<UploadServer>, body: axum::body::Bytes) -> StatusCode {
    state.calls.lock().await.push("upload".into());
    assert_eq!(
        body.as_ref(),
        [b"begin:".as_slice(), NEW_BYTES, b":end"].concat()
    );
    StatusCode::CREATED
}

async fn run_replacement(
    previous_md5: Option<&str>,
    expected_md5: Option<&str>,
    conflict_on: Option<&'static str>,
    exists: bool,
) -> (Result<()>, Vec<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let state = UploadServer {
        base: base.clone(),
        expected_md5: expected_md5.map(String::from),
        conflict_on,
        exists,
        calls: calls.clone(),
    };
    let router = Router::new()
        .route("/users/1/items/ATTACH01/file", post(file_request))
        .route("/upload", post(upload))
        .with_state(state);
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    // Bypass constructor discovery so this fixture exercises only replacement.
    let client = ZoteroClient {
        http: Client::new(),
        base,
        user_id: "1".into(),
        api_key: None,
        basic_auth: None,
        collections: Mutex::new(None),
    };
    let result = client
        .replace_attachment_file("ATTACH01", previous_md5, "syllabus.pdf", None, NEW_BYTES)
        .await;
    server.abort();
    let recorded = calls.lock().await.clone();
    (result, recorded)
}

#[tokio::test]
async fn replacement_uses_previous_md5_at_authorization_and_registration() {
    let (result, calls) = run_replacement(Some(OLD_MD5), Some(OLD_MD5), None, false).await;
    result.unwrap();
    assert_eq!(calls, ["authorize", "upload", "register"]);
}

#[tokio::test]
async fn replacement_of_an_empty_attachment_requires_no_existing_file() {
    for md5 in [None, Some("")] {
        let (result, calls) = run_replacement(md5, None, None, false).await;
        result.unwrap();
        assert_eq!(calls, ["authorize", "upload", "register"]);
    }
}

#[tokio::test]
async fn replacement_stops_if_the_file_changed_before_authorization() {
    let (result, calls) =
        run_replacement(Some(OLD_MD5), Some(OLD_MD5), Some("authorize"), false).await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("412 Precondition Failed"));
    assert_eq!(calls, ["authorize"]);
}

#[tokio::test]
async fn replacement_reports_a_conflict_during_registration_without_retrying() {
    let (result, calls) =
        run_replacement(Some(OLD_MD5), Some(OLD_MD5), Some("register"), false).await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("412 Precondition Failed"));
    assert_eq!(calls, ["authorize", "upload", "register"]);
}

#[tokio::test]
async fn replacement_skips_upload_when_zotero_already_has_the_file() {
    let (result, calls) = run_replacement(Some(OLD_MD5), Some(OLD_MD5), None, true).await;
    result.unwrap();
    assert_eq!(calls, ["authorize"]);
}
