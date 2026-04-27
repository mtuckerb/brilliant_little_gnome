// Optional embedded REST API. Mirrors the Ruby /api/v1 routes.
// Stub: spins up an axum server with a /health endpoint and the /api/v1 namespace
// reserved. Real route coverage lands alongside the matching command modules.

use crate::error::Result;
use crate::state::AppState;
use axum::{routing::get, Json, Router};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;

pub struct RestHandle {
    pub bind: String,
    pub port: i64,
    pub shutdown: oneshot::Sender<()>,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

pub async fn start(state: Arc<AppState>) -> Result<RestHandle> {
    let prefs: (i64, i64) = sqlx::query_as("SELECT api_listen_all, api_port FROM user_preferences LIMIT 1")
        .fetch_one(&state.pool)
        .await?;
    let listen_all = prefs.0 != 0;
    let port = prefs.1;

    let bind_host = if listen_all { "0.0.0.0" } else { "127.0.0.1" };
    let addr: SocketAddr = format!("{}:{}", bind_host, port).parse().map_err(|e: std::net::AddrParseError| crate::error::AppError::Other(e.to_string()))?;

    let app = Router::new()
        .route("/health", get(|| async {
            Json(Health { status: "ok", version: env!("CARGO_PKG_VERSION") })
        }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let (tx, rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        let server = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
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
