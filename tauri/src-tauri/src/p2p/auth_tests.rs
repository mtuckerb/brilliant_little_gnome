//! Local-only regression coverage: real iroh connections, stub Brightspace,
//! real validation and SQLite storage. No accounts or public relays are used.
use super::*;
use crate::client::BrightspaceClient;
use crate::p2p::bridge::BridgeEventSink;
use axum::{
    http::{HeaderMap, StatusCode},
    routing::get,
    Router,
};
use iroh::{
    address_lookup::memory::MemoryLookup, endpoint::presets::Minimal, tls::CaRootsConfig, Endpoint,
    EndpointAddr,
};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use tempfile::TempDir;

const LIVE: &str = "d2lSessionVal=live-session; d2lSecureSessionVal=live-secure";
const EXPIRED: &str = "d2lSessionVal=expired-session; d2lSecureSessionVal=expired-secure";

struct NopSink;
impl BridgeEventSink for NopSink {
    fn course_updated(&self, _: &str) {}
    fn assignments_updated(&self) {}
    fn grades_updated(&self) {}
    fn notifications_updated(&self) {}
    fn prefs_updated(&self) {}
}

async fn validation_server(reject_all: bool) -> (String, AbortOnDropHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/whoami", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/whoami",
        get(move |headers: HeaderMap| async move {
            if !reject_all && headers.get("cookie").and_then(|h| h.to_str().ok()) == Some(LIVE) {
                StatusCode::OK
            } else {
                StatusCode::UNAUTHORIZED
            }
        }),
    );
    let task = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    (url, task)
}

async fn database() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("INSERT INTO user_preferences DEFAULT VALUES")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

async fn exercise_recovery(relay_only: bool) {
    let (relay_map, relay_url, _relay) = iroh::test_utils::run_relay_server().await.unwrap();
    let lookup = MemoryLookup::new();
    let (url, _server) = validation_server(false).await;
    let tmp = TempDir::new().unwrap();
    let mut devices = Vec::new();
    for index in 0..4 {
        let mut builder = Endpoint::builder(Minimal)
            .secret_key(SecretKey::generate())
            .ca_roots_config(CaRootsConfig::insecure_skip_verify());
        builder = if relay_only {
            builder
                .clear_ip_transports()
                .relay_mode(iroh::RelayMode::Custom(relay_map.clone()))
        } else {
            builder.relay_mode(iroh::RelayMode::Disabled)
        };
        let endpoint = builder.bind().await.unwrap();
        endpoint.address_lookup().unwrap().add(lookup.clone());
        if relay_only {
            endpoint.online().await;
            assert_eq!(endpoint.addr().ip_addrs().count(), 0);
            lookup.add_endpoint_info(
                EndpointAddr::new(endpoint.id()).with_relay_url(relay_url.clone()),
            );
        } else {
            lookup.add_endpoint_info(endpoint.addr());
        }
        let pool = database().await;
        let client = Arc::new(BrightspaceClient::for_peer_test(&pool, url.clone()).await);
        // Both donors persist an old cookie. The last donor has a newer,
        // live in-memory cookie, as happens with Brightspace rotation.
        if index > 0 {
            client
                .store_credentials(&pool, "lms.example.edu", EXPIRED, Some("uid"), Some("42"))
                .await
                .unwrap();
        }
        if index == 2 {
            *client.cookie.write() = Some(LIVE.into());
        }
        let transport = Arc::new(
            Transport::start_with_endpoint(
                endpoint,
                if index == 3 {
                    b"other-group"
                } else {
                    b"paired-group"
                },
                vec![],
            )
            .await
            .unwrap(),
        );
        let doc = Arc::new(SyncDoc::new());
        let bridge = Bridge::with_sql(doc.clone(), pool.clone(), Arc::new(NopSink));
        bridge.set_client(client.clone());
        let store = Arc::new(SyncStore::open_at(tmp.path().join(index.to_string())).unwrap());
        let engine = SyncEngine::start_with_parts(store, doc, transport, bridge)
            .await
            .unwrap();
        devices.push((engine, pool, client));
    }
    let (requester, pool, client) = &devices[0];
    assert!(!requester
        .recover_credentials_from_peers(pool, client)
        .await
        .unwrap());
    for (index, (engine, _, _)) in devices.iter().enumerate().take(3).skip(1) {
        sqlx::query("INSERT INTO paired_devices (id, public_key, last_seen_at) VALUES (?, ?, ?)")
            .bind(engine.endpoint_id().to_string())
            .bind(engine.endpoint_id().to_string())
            .bind(if index == 1 {
                "2026-09-23"
            } else {
                "2026-09-22"
            })
            .execute(pool)
            .await
            .unwrap();
    }
    assert!(
        !requester.transport.has_connected_peers(),
        "recovery must not depend on gossip neighbors"
    );
    assert!(!client.is_configured());
    // Expired first donor must be skipped; the last donor supplies its live
    // value, even though its persisted cookie is stale.
    assert!(requester
        .recover_credentials_from_peers(pool, client)
        .await
        .unwrap());
    assert_eq!(client.cookie_clone().as_deref(), Some(LIVE));
    assert_eq!(client.user_id_clone().as_deref(), Some("42"));
    let saved: String = sqlx::query_scalar("SELECT brightspace_cookie FROM user_preferences")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(saved, LIVE);

    client
        .store_credentials(pool, "lms.example.edu", EXPIRED, None, None)
        .await
        .unwrap();
    *client.degraded.write() = true;
    assert!(requester
        .recover_credentials_from_peers(pool, client)
        .await
        .unwrap());
    assert!(!client.is_degraded());

    // Even a donor's confirmed-live response cannot bypass the recipient's
    // own validation. Failure must leave the existing session unchanged.
    let (reject_url, _reject_server) = validation_server(true).await;
    let rejecting_client = BrightspaceClient::for_peer_test(pool, reject_url).await;
    assert!(!requester
        .recover_credentials_from_peers(pool, &rejecting_client)
        .await
        .unwrap());
    assert_eq!(rejecting_client.cookie_clone().as_deref(), Some(LIVE));
    let saved: String = sqlx::query_scalar("SELECT brightspace_cookie FROM user_preferences")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(saved, LIVE);

    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unreachable_url = format!("http://{}/whoami", closed.local_addr().unwrap());
    drop(closed);
    let unreachable_client = BrightspaceClient::for_peer_test(pool, unreachable_url).await;
    assert!(!requester
        .recover_credentials_from_peers(pool, &unreachable_client)
        .await
        .unwrap());
    assert_eq!(unreachable_client.cookie_clone().as_deref(), Some(LIVE));

    // Endpoint identity alone is insufficient: a device from a different
    // (or rotated-out) sync group cannot use the credential protocol.
    assert!(requester
        .transport
        .send_credentials(
            devices[3].0.endpoint_id(),
            WireMsg::CredentialRequest {
                request_id: "wrong-group".into(),
                to: devices[3].0.endpoint_id().to_string(),
            }
        )
        .await
        .is_err());

    // No request or cookie can fall back to a group broadcast.
    assert!(requester
        .transport
        .broadcast(WireMsg::CredentialRequest {
            request_id: "not-broadcast".into(),
            to: devices[2].0.endpoint_id().to_string(),
        })
        .await
        .is_err());

    for (engine, _, _) in devices {
        engine.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn peer_auth_recovers_missing_and_expired_sessions_over_lan() {
    tokio::time::timeout(Duration::from_secs(45), exercise_recovery(false))
        .await
        .unwrap();
}

#[tokio::test]
async fn peer_auth_recovers_missing_and_expired_sessions_over_relay_only() {
    tokio::time::timeout(Duration::from_secs(45), exercise_recovery(true))
        .await
        .unwrap();
}
