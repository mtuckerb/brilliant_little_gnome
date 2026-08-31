// Optional embedded REST API for external integrations.
// Auth: `Authorization: Bearer <api_key-or-JWT>`, `X-API-Key: <api_key>`, or
// `?api_key=…`. The static key and API enablement live in user_preferences.
// `/health`, `/docs`, `/openapi.yaml`, and `/api/v1/auth/cookies` are public.

use crate::auth;
use crate::error::{AppError, Result};
use crate::state::AppState;
use axum::{
    body::Body,
    extract::{Path, Query, State as AxState},
    http::{header, HeaderMap, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tower_http::cors::{Any, CorsLayer};

pub struct RestHandle {
    pub bind: String,
    pub port: i64,
    pub shutdown: oneshot::Sender<()>,
}

pub async fn start(state: Arc<AppState>) -> Result<RestHandle> {
    let prefs: (i64, i64) =
        sqlx::query_as("SELECT api_listen_all, api_port FROM user_preferences LIMIT 1")
            .fetch_one(&state.pool)
            .await?;
    let listen_all = prefs.0 != 0;
    let port = prefs.1;

    let bind_host = if listen_all { "0.0.0.0" } else { "127.0.0.1" };
    let addr: SocketAddr = format!("{}:{}", bind_host, port)
        .parse()
        .map_err(|e: std::net::AddrParseError| AppError::Other(e.to_string()))?;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    // Routes that bypass api-key auth: /health, /auth/cookies (used by the
    // browser-extension re-auth flow).
    let open = Router::new()
        .route("/health", get(health))
        .route("/docs", get(swagger_ui))
        .route("/openapi.yaml", get(openapi_spec))
        .route("/api/v1/auth/cookies", post(auth_cookies));

    let protected = Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/preferences", get(preferences))
        .route("/api/v1/token", get(token))
        .route("/api/v1/courses", get(list_courses))
        .route("/api/v1/courses/reorder", post(reorder_courses))
        .route("/api/v1/courses/:id", get(get_course))
        .route("/api/v1/courses/:id/export", get(export_course))
        .route("/api/v1/courses/:id/grades/summary", get(grades_summary))
        .route(
            "/api/v1/courses/:id/assignments/summary",
            get(assignments_summary),
        )
        .route("/api/v1/courses/:id/discussions", get(course_discussions))
        .route("/api/v1/notifications", get(notifications))
        .route("/api/v1/dashboard/summary", get(dashboard_summary))
        .route("/api/v1/search", get(global_search))
        .route("/api/v1/assignments", post(create_assignment))
        // Native MCP (Model Context Protocol) server over Streamable HTTP, so
        // agents get Brilliant as first-class tools. Same bearer auth.
        .route("/mcp", post(mcp_handle))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_key,
        ));

    let app = Router::new()
        .merge(open)
        .merge(protected)
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let (tx, rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            rx.await.ok();
        });
        if let Err(e) = server.await {
            tracing::warn!("rest api server error: {}", e);
        }
    });

    Ok(RestHandle {
        bind: bind_host.to_string(),
        port,
        shutdown: tx,
    })
}

// ---- middleware --------------------------------------------------------

async fn require_api_key(
    AxState(state): AxState<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> std::result::Result<Response, Response> {
    let prefs: (i64, Option<String>, Option<String>) =
        sqlx::query_as("SELECT api_enabled, api_key, jwt_secret FROM user_preferences LIMIT 1")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if prefs.0 == 0 {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "API access is disabled".into(),
        ));
    }
    if prefs.1.as_deref().unwrap_or_default().is_empty()
        && prefs.2.as_deref().unwrap_or_default().is_empty()
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "API key not configured".into(),
        ));
    }
    let provided = extract_credential(req.headers(), req.uri().query())
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "missing API credential".into()))?;
    if !credential_is_valid(&provided, prefs.1.as_deref(), prefs.2.as_deref()) {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "invalid API credential".into(),
        ));
    }
    Ok(next.run(req).await)
}

fn extract_credential(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(h) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(rest) = h.strip_prefix("Bearer ") {
            return Some(rest.to_string());
        }
    }
    if let Some(h) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        if !h.is_empty() {
            return Some(h.to_string());
        }
    }
    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "api_key" {
                    return Some(urldecode(v));
                }
            }
        }
    }
    None
}

fn credential_is_valid(provided: &str, api_key: Option<&str>, jwt_secret: Option<&str>) -> bool {
    auth::verify_api_key(provided, api_key).is_ok()
        || jwt_secret
            .filter(|secret| !secret.is_empty())
            .is_some_and(|secret| auth::verify(secret, provided).is_ok())
}

fn urldecode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

fn api_error(code: StatusCode, msg: String) -> Response {
    let body = json!({ "error": msg }).to_string();
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

const OPENAPI_YAML: &str = include_str!("../../../docs/openapi.yaml");
const SWAGGER_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Brilliant API Documentation</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    SwaggerUIBundle({
      url: "/openapi.yaml",
      dom_id: "#swagger-ui",
      deepLinking: true,
      persistAuthorization: true,
      layout: "BaseLayout"
    });
  </script>
</body>
</html>
"##;

async fn swagger_ui() -> Html<&'static str> {
    Html(SWAGGER_HTML)
}

async fn openapi_spec() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/yaml; charset=utf-8")
        .body(Body::from(OPENAPI_YAML))
        .unwrap()
}

// ---- MCP (Model Context Protocol) over Streamable HTTP -------------------
//
// A minimal, dependency-free MCP server sharing this axum app + bearer auth.
// JSON-RPC 2.0. Handles `initialize`, `tools/list`, `tools/call`, `ping`, and
// silently 202s notifications. Tools reuse the REST handlers above so there's
// one source of truth for the data.

const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

async fn mcp_handle(AxState(state): AxState<Arc<AppState>>, Json(req): Json<Value>) -> Response {
    let method = req
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    // A JSON-RPC message with no `id` is a notification — ack with 202, no body.
    let Some(id) = req.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };

    let result: std::result::Result<Value, (i64, String)> = match method.as_str() {
        "initialize" => Ok(mcp_initialize_result()),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": mcp_tool_defs() })),
        "tools/call" => mcp_tools_call(&state, &params).await,
        other => Err((-32601, format!("Method not found: {other}"))),
    };

    let payload = match result {
        Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    };
    Json(payload).into_response()
}

fn mcp_initialize_result() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "brilliant", "version": env!("CARGO_PKG_VERSION") }
    })
}

fn mcp_tool_defs() -> Value {
    let course_arg = json!({
        "type": "object",
        "properties": { "course_id": { "type": "string", "description": "course org_unit_id" } },
        "required": ["course_id"]
    });
    json!([
        { "name": "list_courses", "description": "List all Brightspace courses (org_unit_id, code, name, semester, status, pins).", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "get_course", "description": "Get one course by org_unit_id.", "inputSchema": course_arg },
        { "name": "grades_summary", "description": "Grade rows + roll-up (earned/possible, projected) for a course.", "inputSchema": course_arg },
        { "name": "assignments_summary", "description": "Assignment list + due dates for a course.", "inputSchema": course_arg },
        { "name": "list_discussions", "description": "Discussion topics, thread counts, post counts, and last-post dates for a course.", "inputSchema": course_arg },
        { "name": "list_notifications", "description": "Recent notifications / course announcements.", "inputSchema": { "type": "object", "properties": { "limit": { "type": "integer" } } } },
        { "name": "dashboard_summary", "description": "Cross-course dashboard summary.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "search", "description": "Search courses, assignments, content, discussions, and notifications.", "inputSchema": {
            "type": "object",
            "properties": { "query": { "type": "string", "description": "Search text" } },
            "required": ["query"]
        } },
        { "name": "export_course", "description": "Export a whole course (module tree + files, plus Markdown for content pages, overview, and announcements) as a zip saved to dest_dir. Returns the saved path. Unzip to browse in a vault.", "inputSchema": {
            "type": "object",
            "properties": {
                "course_id": { "type": "string", "description": "course org_unit_id" },
                "dest_dir": { "type": "string", "description": "absolute directory to write the .zip into" }
            },
            "required": ["course_id", "dest_dir"]
        } }
    ])
}

async fn mcp_tools_call(
    state: &Arc<AppState>,
    params: &Value,
) -> std::result::Result<Value, (i64, String)> {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let cid = args
        .get("course_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let need_cid = || cid.clone().ok_or_else(|| "course_id required".to_string());

    let data: std::result::Result<Value, String> = match name {
        "list_courses" => unwrap_json(list_courses(AxState(state.clone())).await),
        "dashboard_summary" => unwrap_json(dashboard_summary(AxState(state.clone())).await),
        "get_course" => match need_cid() {
            Ok(id) => unwrap_json(get_course(AxState(state.clone()), Path(id)).await),
            Err(e) => Err(e),
        },
        "grades_summary" => match need_cid() {
            Ok(id) => unwrap_json(
                grades_summary(AxState(state.clone()), Path(id), Query(HashMap::new())).await,
            ),
            Err(e) => Err(e),
        },
        "assignments_summary" => match need_cid() {
            Ok(id) => unwrap_json(
                assignments_summary(AxState(state.clone()), Path(id), Query(HashMap::new())).await,
            ),
            Err(e) => Err(e),
        },
        "list_discussions" => match need_cid() {
            Ok(id) => unwrap_json(course_discussions(AxState(state.clone()), Path(id)).await),
            Err(e) => Err(e),
        },
        "list_notifications" => {
            let mut q = HashMap::new();
            if let Some(l) = args.get("limit").and_then(|v| v.as_i64()) {
                q.insert("limit".to_string(), l.to_string());
            }
            unwrap_json(notifications(AxState(state.clone()), Query(q)).await)
        }
        "search" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| "query required".to_string());
            match query {
                Ok(query) => {
                    let mut q = HashMap::new();
                    q.insert("q".to_string(), query.to_string());
                    unwrap_json(global_search(AxState(state.clone()), Query(q)).await)
                }
                Err(e) => Err(e),
            }
        }
        "export_course" => match (cid.clone(), args.get("dest_dir").and_then(|v| v.as_str())) {
            (Some(id), Some(dest)) => mcp_export_course(state, &id, dest).await,
            (None, _) => Err("course_id required".to_string()),
            (_, None) => Err("dest_dir required".to_string()),
        },
        other => return Err((-32602, format!("Unknown tool: {other}"))),
    };

    Ok(match data {
        Ok(v) => {
            let text = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
        }
        Err(msg) => {
            json!({ "content": [{ "type": "text", "text": format!("Error: {msg}") }], "isError": true })
        }
    })
}

/// Pull the `Value` out of a REST handler's `Result<Json<Value>, Response>`.
fn unwrap_json(
    r: std::result::Result<Json<Value>, Response>,
) -> std::result::Result<Value, String> {
    match r {
        Ok(Json(v)) => Ok(v),
        Err(_) => Err("tool call failed (check the app log)".to_string()),
    }
}

async fn mcp_export_course(
    state: &Arc<AppState>,
    course_id: &str,
    dest_dir: &str,
) -> std::result::Result<Value, String> {
    let (bytes, failures) = crate::commands::downloads::build_course_archive(state, course_id)
        .await
        .map_err(|e| e.to_string())?;
    let dir = std::path::Path::new(dest_dir);
    std::fs::create_dir_all(dir).map_err(|e| format!("create {dest_dir}: {e}"))?;
    let path = dir.join(format!("Brilliant-{course_id}.zip"));
    std::fs::write(&path, &bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(json!({
        "saved_path": path.display().to_string(),
        "bytes": bytes.len(),
        "warnings": failures.len(),
        "note": "zip bundle written; unzip to browse (module tree + files + _Overview.md + _Announcements.md)"
    }))
}

// ---- handlers ----------------------------------------------------------

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn status(AxState(state): AxState<Arc<AppState>>) -> Json<Value> {
    let sync_status = state.sync_status.read().clone();
    Json(json!({
        "authenticated": state.client.is_configured(),
        "degraded_mode": state.client.is_degraded(),
        "host": state.client.host_clone(),
        "sync_status": sync_status,
    }))
}

async fn preferences(
    AxState(state): AxState<Arc<AppState>>,
) -> std::result::Result<Json<Value>, Response> {
    let row: Value = sqlx::query_scalar(
        "SELECT json_object(
            'id', id,
            'display_name', display_name,
            'time_zone', time_zone,
            'brightspace_host', brightspace_host,
            'api_enabled', api_enabled,
            'api_listen_all', api_listen_all,
            'api_port', api_port,
            'historic_gpa', historic_gpa,
            'historic_units', historic_units,
            'default_semester', default_semester,
            'brightspace_uid', brightspace_uid,
            'brightspace_user_id', brightspace_user_id,
            'last_login_at', last_login_at
         ) FROM user_preferences LIMIT 1",
    )
    .fetch_one(&state.pool)
    .await
    .map(|s: String| serde_json::from_str(&s).unwrap_or(Value::Null))
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(row))
}

async fn token(
    AxState(state): AxState<Arc<AppState>>,
) -> std::result::Result<Json<Value>, Response> {
    let secret = auth::ensure_secret(&state.pool)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let display: Option<String> =
        sqlx::query_scalar("SELECT display_name FROM user_preferences LIMIT 1")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(None);
    let t = auth::issue(
        &secret,
        display.as_deref().unwrap_or("internal"),
        60 * 60 * 24,
    )
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "token": t })))
}

async fn list_courses(
    AxState(state): AxState<Arc<AppState>>,
) -> std::result::Result<Json<Value>, Response> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT json_object(
            'org_unit_id', org_unit_id, 'name', COALESCE(custom_name, name), 'custom_name', custom_name, 'code', code, 'semester', semester,
            'is_pinned', is_pinned, 'custom_color', custom_color, 'banner_url', banner_url,
            'units', units, 'target_grade', target_grade, 'status', status,
            'sort_order', sort_order, 'last_accessed_at', last_accessed_at
         ) FROM courses ORDER BY is_pinned DESC, sort_order ASC, last_accessed_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let arr: Vec<Value> = rows
        .into_iter()
        .map(|(j,)| serde_json::from_str(&j).unwrap_or(Value::Null))
        .collect();
    Ok(Json(Value::Array(arr)))
}

/// Stream the whole course as a zip (module tree + files, with HTML content
/// pages / overview / announcements converted to Markdown) so an external
/// integration like gbrain can fetch it daily and unpack it into an Obsidian
/// vault. Same builder the in-app "Download All" uses.
async fn export_course(
    AxState(state): AxState<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<Response, Response> {
    match crate::commands::downloads::build_course_archive(&state, &id).await {
        Ok((bytes, _failures)) => {
            let mut resp = Response::new(Body::from(bytes));
            resp.headers_mut()
                .insert(header::CONTENT_TYPE, "application/zip".parse().unwrap());
            if let Ok(cd) = format!("attachment; filename=\"Brilliant-{}.zip\"", id).parse() {
                resp.headers_mut().insert(header::CONTENT_DISPOSITION, cd);
            }
            Ok(resp)
        }
        Err(e) => Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn get_course(
    AxState(state): AxState<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<Json<Value>, Response> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT json_object(
            'org_unit_id', org_unit_id, 'name', COALESCE(custom_name, name), 'custom_name', custom_name, 'code', code, 'semester', semester,
            'is_pinned', is_pinned, 'custom_color', custom_color, 'banner_url', banner_url,
            'units', units, 'target_grade', target_grade, 'status', status,
            'sort_order', sort_order, 'last_accessed_at', last_accessed_at
         ) FROM courses WHERE org_unit_id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    match row {
        Some((j,)) => Ok(Json(serde_json::from_str(&j).unwrap_or(Value::Null))),
        None => Err(api_error(StatusCode::NOT_FOUND, "Course not found".into())),
    }
}

#[derive(Deserialize)]
struct ReorderBody {
    ids: Vec<String>,
}

async fn reorder_courses(
    AxState(state): AxState<Arc<AppState>>,
    Json(body): Json<ReorderBody>,
) -> std::result::Result<Json<Value>, Response> {
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    for (i, id) in body.ids.iter().enumerate() {
        sqlx::query("UPDATE courses SET sort_order = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
            .bind(i as i64)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    tx.commit()
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "status": "ok" })))
}

async fn grades_summary(
    AxState(state): AxState<Arc<AppState>>,
    Path(id): Path<String>,
    Query(_q): Query<HashMap<String, String>>,
) -> std::result::Result<Json<Value>, Response> {
    // Reuse the Tauri command logic.
    use crate::commands::grades::grades_summary as cmd;
    // We don't have AppStateArg<'_> in this context, so reimplement against pool directly.
    let _ = cmd;
    let target: Option<(Option<f64>,)> =
        sqlx::query_as("SELECT target_grade FROM courses WHERE org_unit_id = ?")
            .bind(&id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let target = target.and_then(|t| t.0).unwrap_or(93.0);

    let grades: Vec<(i64, String, Option<String>, String, Option<String>, Option<f64>, Option<f64>, Option<f64>, Option<String>, i64, i64, i64, Option<f64>, Option<String>)> = sqlx::query_as(
        "SELECT id, course_id, brightspace_id, name, displayed_grade, numerator, denominator, weight, due_date, is_extra_credit, hidden, manually_marked_ungraded, expected_score, comments
         FROM grades WHERE course_id = ? ORDER BY due_date ASC NULLS LAST, name ASC",
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut earned = 0.0;
    let mut possible = 0.0;
    let mut all_possible = 0.0;
    let mut rows: Vec<Value> = Vec::with_capacity(grades.len());
    for g in &grades {
        if g.10 != 0 {
            continue;
        } // hidden
        let denom = g.6.unwrap_or(0.0);
        all_possible += denom;
        let is_graded = g.11 == 0 && g.5.is_some() && denom > 0.0;
        let is_expected = !is_graded && g.12.is_some() && denom > 0.0;
        let perc = if is_graded {
            Some(g.5.unwrap_or(0.0) / denom * 100.0)
        } else if is_expected {
            g.12
        } else {
            None
        };
        if is_graded {
            earned += g.5.unwrap_or(0.0);
            possible += denom;
        } else if is_expected {
            earned += denom * (g.12.unwrap_or(0.0) / 100.0);
            possible += denom;
        }
        rows.push(json!({
            "id": g.0, "course_id": g.1, "brightspace_id": g.2, "name": g.3,
            "displayed_grade": g.4, "numerator": g.5, "denominator": g.6, "weight": g.7,
            "due_date": g.8, "is_extra_credit": g.9 != 0, "hidden": g.10 != 0,
            "manually_marked_ungraded": g.11 != 0, "expected_score": g.12, "comments": g.13,
            "perc": perc, "is_graded": is_graded, "is_expected": is_expected,
            "rel_weight": if all_possible > 0.0 { denom / all_possible } else { 0.0 },
        }));
    }
    let score = if possible > 0.0 {
        Some(earned / possible * 100.0)
    } else {
        None
    };
    let remaining = (all_possible - possible).max(0.0);
    let needed_total = all_possible * (target / 100.0);
    let required_avg = if remaining > 0.0 {
        Some(((needed_total - earned) / remaining * 100.0).max(0.0))
    } else {
        None
    };
    let is_impossible = required_avg.map(|r| r > 100.0).unwrap_or(false);

    Ok(Json(json!({
        "grades": rows,
        "grade_stats": {
            "score": score,
            "confidence": if all_possible > 0.0 { possible / all_possible } else { 0.0 },
            "total_points_earned": earned,
            "total_points_possible": possible,
            "all_possible_points": all_possible,
            "remaining_points": remaining,
            "target_grade": target,
            "required_avg": required_avg,
            "is_impossible": is_impossible,
        }
    })))
}

async fn assignments_summary(
    AxState(state): AxState<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> std::result::Result<Json<Value>, Response> {
    let show_completed = q
        .get("show_completed")
        .map(|s| s == "true")
        .unwrap_or(false);
    let sql = if show_completed {
        "SELECT json_object(
            'id', id, 'course_id', course_id, 'brightspace_id', brightspace_id,
            'name', name, 'due_date', due_date, 'description', description,
            'is_graded', is_graded, 'grade_item_id', grade_item_id,
            'assignment_type', assignment_type, 'completed', completed,
            'completed_at', completed_at, 'synthetic', synthetic,
            'optional', optional, 'external_url', external_url,
            'is_quiz', assignment_type = 'quiz'
         ) FROM assignments WHERE course_id = ? ORDER BY due_date IS NULL, COALESCE(datetime(due_date), due_date) ASC"
    } else {
        "SELECT json_object(
            'id', id, 'course_id', course_id, 'brightspace_id', brightspace_id,
            'name', name, 'due_date', due_date, 'description', description,
            'is_graded', is_graded, 'grade_item_id', grade_item_id,
            'assignment_type', assignment_type, 'completed', completed,
            'completed_at', completed_at, 'synthetic', synthetic,
            'optional', optional, 'external_url', external_url,
            'is_quiz', assignment_type = 'quiz'
         ) FROM assignments WHERE course_id = ? AND completed = 0 ORDER BY due_date IS NULL, COALESCE(datetime(due_date), due_date) ASC"
    };
    let rows: Vec<(String,)> = sqlx::query_as(sql)
        .bind(&id)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let arr: Vec<Value> = rows
        .into_iter()
        .map(|(j,)| serde_json::from_str(&j).unwrap_or(Value::Null))
        .collect();
    Ok(Json(json!({ "assignments": arr })))
}

async fn course_discussions(
    AxState(state): AxState<Arc<AppState>>,
    Path(id): Path<String>,
) -> std::result::Result<Json<Value>, Response> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT json_object(
            'id', id,
            'brightspace_id', brightspace_id,
            'course_id', course_id,
            'forum_id', forum_id,
            'name', name,
            'description', description,
            'sort_order', sort_order,
            'thread_count', thread_count,
            'post_count', post_count,
            'last_post_date', last_post_date,
            'display_thread_count', thread_count,
            'display_post_count', post_count
         )
         FROM discussion_topics
         WHERE course_id = ?
         ORDER BY last_post_date DESC NULLS LAST, sort_order ASC, name ASC",
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let topics = rows
        .into_iter()
        .map(|(row,)| serde_json::from_str(&row).unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "topics": topics })))
}

async fn global_search(
    AxState(state): AxState<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> std::result::Result<Json<Value>, Response> {
    let query = q.get("q").map(|v| v.trim()).unwrap_or_default();
    if query.is_empty() {
        return Ok(Json(json!({ "results": [] })));
    }
    let pattern = format!("%{}%", escape_like(query.to_lowercase()));
    let mut results = Vec::new();

    let courses: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT org_unit_id, COALESCE(custom_name, name), code
         FROM courses
         WHERE lower(name) LIKE ? ESCAPE '\\'
            OR lower(COALESCE(custom_name, '')) LIKE ? ESCAPE '\\'
            OR lower(COALESCE(code, '')) LIKE ? ESCAPE '\\'
         ORDER BY is_pinned DESC, sort_order ASC
         LIMIT 5",
    )
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    results.extend(courses.into_iter().map(|(course_id, title, code)| {
        json!({
            "type": "course",
            "title": title,
            "url": format!("/course/{course_id}"),
            "subtitle": code,
            "course_id": course_id
        })
    }));

    let assignments: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT a.course_id, a.brightspace_id, a.name, COALESCE(c.custom_name, c.name)
         FROM assignments a
         LEFT JOIN courses c ON c.org_unit_id = a.course_id
         WHERE lower(a.name) LIKE ? ESCAPE '\\'
            OR lower(COALESCE(a.description, '')) LIKE ? ESCAPE '\\'
         ORDER BY a.due_date DESC NULLS LAST
         LIMIT 10",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    results.extend(
        assignments
            .into_iter()
            .map(|(course_id, assignment_id, title, course)| {
                json!({
                    "type": "assignment",
                    "title": title,
                    "url": format!("/course/{course_id}/assignments/{assignment_id}"),
                    "subtitle": course,
                    "course_id": course_id
                })
            }),
    );

    let content: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT m.course_id, i.module_id, i.title, m.title
         FROM content_items i
         JOIN content_modules m ON m.brightspace_id = i.module_id
         WHERE i.is_hidden = 0 AND lower(i.title) LIKE ? ESCAPE '\\'
         ORDER BY m.sort_order ASC, i.sort_order ASC
         LIMIT 10",
    )
    .bind(&pattern)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    results.extend(
        content
            .into_iter()
            .map(|(course_id, module_id, title, module)| {
                json!({
                    "type": "content",
                    "title": title,
                    "url": format!("/course/{course_id}/module/{module_id}"),
                    "subtitle": module.map(|v| format!("Module: {v}")),
                    "course_id": course_id
                })
            }),
    );

    let discussions: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT t.course_id, t.forum_id, t.brightspace_id, t.name, t.last_post_date
         FROM discussion_topics t
         WHERE lower(t.name) LIKE ? ESCAPE '\\'
            OR lower(COALESCE(t.description, '')) LIKE ? ESCAPE '\\'
         ORDER BY t.last_post_date DESC NULLS LAST
         LIMIT 10",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    results.extend(discussions.into_iter().map(
        |(course_id, forum_id, topic_id, title, last_post_date)| {
            json!({
                "type": "discussion",
                "title": title,
                "url": format!("/course/{course_id}/discussions/{forum_id}/topics/{topic_id}"),
                "subtitle": last_post_date,
                "course_id": course_id
            })
        },
    ));

    let notifications: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, title, course_name, course_id
         FROM notifications
         WHERE lower(title) LIKE ? ESCAPE '\\'
            OR lower(COALESCE(body, '')) LIKE ? ESCAPE '\\'
         ORDER BY date DESC NULLS LAST
         LIMIT 10",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    results.extend(
        notifications
            .into_iter()
            .map(|(id, title, course, course_id)| {
                json!({
                    "type": "notification",
                    "title": title,
                    "url": format!("/notifications/{id}/view"),
                    "subtitle": course,
                    "course_id": course_id
                })
            }),
    );

    Ok(Json(json!({ "results": results })))
}

fn escape_like(value: String) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

async fn notifications(
    AxState(state): AxState<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> std::result::Result<Json<Value>, Response> {
    let limit: i64 = q
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(25)
        .min(200)
        .max(1);
    let offset: i64 = q
        .get("offset")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
        .max(0);
    let show_read = q.get("show_read").map(|s| s == "true").unwrap_or(false);

    let mut clauses: Vec<&str> = Vec::new();
    if !show_read {
        clauses.push("n.is_read = 0");
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };

    let total_sql = format!("SELECT COUNT(*) FROM notifications n {}", where_clause);
    let total: i64 = sqlx::query_scalar(&total_sql)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let list_sql = format!(
        "SELECT json_object(
            'id', id, 'external_id', external_id, 'notification_type', notification_type,
            'title', title, 'body', body, 'date', date,
            'course_id', n.course_id, 'course_name', COALESCE(c.custom_name, c.name, n.course_name),
            'urgency', n.urgency, 'is_personal', n.is_personal != 0,
            'is_read', n.is_read != 0, 'url', n.url
         ) FROM notifications n LEFT JOIN courses c ON c.org_unit_id = n.course_id {} ORDER BY n.date DESC NULLS LAST, n.id DESC LIMIT ? OFFSET ?",
        where_clause
    );
    let rows: Vec<(String,)> = sqlx::query_as(&list_sql)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let arr: Vec<Value> = rows
        .into_iter()
        .map(|(j,)| serde_json::from_str(&j).unwrap_or(Value::Null))
        .collect();

    Ok(Json(json!({
        "total": total, "limit": limit, "offset": offset, "notifications": arr
    })))
}

async fn dashboard_summary(
    AxState(state): AxState<Arc<AppState>>,
) -> std::result::Result<Json<Value>, Response> {
    let courses: Vec<(String,)> = sqlx::query_as(
        "SELECT json_object(
            'org_unit_id', org_unit_id, 'name', COALESCE(custom_name, name), 'custom_name', custom_name, 'code', code, 'semester', semester,
            'is_pinned', is_pinned, 'custom_color', custom_color, 'banner_url', banner_url
         ) FROM courses ORDER BY is_pinned DESC, sort_order ASC, last_accessed_at DESC LIMIT 100",
    )
    .fetch_all(&state.pool).await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let courses: Vec<Value> = courses
        .into_iter()
        .map(|(j,)| serde_json::from_str(&j).unwrap_or(Value::Null))
        .collect();

    let upcoming: Vec<(String,)> = sqlx::query_as(
        "SELECT json_object(
            'id', id, 'course_id', course_id, 'name', name, 'due_date', due_date,
            'optional', optional, 'completed', completed, 'external_url', external_url
         ) FROM assignments
         WHERE completed = 0 AND due_date IS NOT NULL
           AND due_date > datetime('now')
           AND due_date <= datetime('now', '+14 days')
         ORDER BY optional ASC, due_date ASC LIMIT 15",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let upcoming: Vec<Value> = upcoming
        .into_iter()
        .map(|(j,)| serde_json::from_str(&j).unwrap_or(Value::Null))
        .collect();

    let recent: Vec<(String,)> = sqlx::query_as(
        "SELECT json_object(
            'id', n.id, 'title', n.title, 'body', n.body, 'date', n.date,
            'course_id', n.course_id, 'course_name', COALESCE(c.custom_name, c.name, n.course_name), 'url', n.url
         ) FROM notifications n LEFT JOIN courses c ON c.org_unit_id = n.course_id WHERE n.is_read = 0 ORDER BY n.date DESC LIMIT 10",
    )
    .fetch_all(&state.pool).await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let recent: Vec<Value> = recent
        .into_iter()
        .map(|(j,)| serde_json::from_str(&j).unwrap_or(Value::Null))
        .collect();

    Ok(Json(json!({
        "courses": courses,
        "upcoming_assignments": upcoming,
        "recent_notifications": recent,
    })))
}

#[derive(Deserialize)]
struct AuthCookiesBody {
    host: String,
    cookies: String,
}

async fn auth_cookies(
    AxState(state): AxState<Arc<AppState>>,
    Json(body): Json<AuthCookiesBody>,
) -> std::result::Result<Json<Value>, Response> {
    if body.host.is_empty() || body.cookies.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Missing parameters".into(),
        ));
    }
    state
        .client
        .store_credentials(
            &state.pool,
            body.host.trim(),
            body.cookies.trim(),
            None,
            None,
        )
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    // Pre-share validation gates this REST path too: the key is stored
    // locally above, but only shared to peers if it validates as live.
    // A blocked share returns an actionable, key-free 409 rather than
    // silently mirroring an unvalidated key.
    #[cfg(feature = "p2p")]
    state
        .mirror_credentials_to_loro()
        .await
        .map_err(|e| api_error(StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({ "status": "ok" })))
}

#[derive(Deserialize)]
struct CreateAssignmentBody {
    course_id: String,
    name: String,
    description: Option<String>,
    due_date: Option<String>,
    due_time: Option<String>,
    external_url: Option<String>,
}

async fn create_assignment(
    AxState(state): AxState<Arc<AppState>>,
    Json(body): Json<CreateAssignmentBody>,
) -> std::result::Result<Json<Value>, Response> {
    if body.course_id.is_empty() || body.name.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "course_id and name are required".into(),
        ));
    }
    let bs_id = format!("syn_{}", uuid::Uuid::new_v4().simple());
    let due = match body.due_date {
        None => None,
        Some(d) => match body.due_time {
            Some(t) => Some(format!("{} {}", d, t)),
            None => Some(format!("{}T23:59:59Z", d)),
        },
    };
    let result = sqlx::query(
        "INSERT INTO assignments (course_id, brightspace_id, name, description, due_date, external_url, synthetic, manually_edited, manually_edited_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 1, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(&body.course_id)
    .bind(&bs_id)
    .bind(&body.name)
    .bind(body.description)
    .bind(due)
    .bind(body.external_url)
    .execute(&state.pool)
    .await
    .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(
        json!({ "status": "ok", "id": result.last_insert_rowid(), "brightspace_id": bs_id }),
    ))
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Unauthenticated => StatusCode::UNAUTHORIZED,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        api_error(status, self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_all_documented_api_credentials() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer bearer-value".parse().unwrap(),
        );
        assert_eq!(
            extract_credential(&headers, Some("api_key=query-value")),
            Some("bearer-value".into())
        );

        headers.remove(header::AUTHORIZATION);
        headers.insert("x-api-key", "header-value".parse().unwrap());
        assert_eq!(
            extract_credential(&headers, Some("api_key=query-value")),
            Some("header-value".into())
        );

        headers.remove("x-api-key");
        assert_eq!(
            extract_credential(&headers, Some("other=1&api_key=query%20value")),
            Some("query value".into())
        );
    }

    #[test]
    fn accepts_static_api_keys_and_issued_jwts() {
        assert!(credential_is_valid("static-key", Some("static-key"), None));
        assert!(!credential_is_valid("wrong", Some("static-key"), None));

        let secret = "test-jwt-secret";
        let token = auth::issue(secret, "rest-api-test", 60).unwrap();
        assert!(credential_is_valid(
            &token,
            Some("static-key"),
            Some(secret)
        ));
        assert!(!credential_is_valid(
            &token,
            Some("static-key"),
            Some("other-secret")
        ));
    }

    #[test]
    fn openapi_is_the_tauri_2_contract() {
        assert!(OPENAPI_YAML.contains("version: 2.0.0"));
        assert!(OPENAPI_YAML.contains("/api/v1/search:"));
        assert!(OPENAPI_YAML.contains("/api/v1/courses/{id}/discussions:"));
        assert!(OPENAPI_YAML.contains("/mcp:"));
        assert!(OPENAPI_YAML.contains("X-API-Key"));
    }

    #[test]
    fn mcp_initialize_and_tool_catalog_are_complete() {
        let initialized = mcp_initialize_result();
        assert_eq!(initialized["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(initialized["serverInfo"]["name"], "brilliant");

        let tools = mcp_tool_defs();
        let names = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "list_courses",
                "get_course",
                "grades_summary",
                "assignments_summary",
                "list_discussions",
                "list_notifications",
                "dashboard_summary",
                "search",
                "export_course",
            ]
        );
    }

    #[test]
    fn search_escapes_like_wildcards() {
        assert_eq!(escape_like(r"100%_done\ok".into()), r"100\%\_done\\ok");
    }
}
