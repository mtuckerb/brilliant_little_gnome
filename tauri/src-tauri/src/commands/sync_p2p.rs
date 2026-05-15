// P2P sync Tauri command surface (T-014, design.md §8).
//
// All commands are gated on the `p2p` feature. The non-p2p build
// (no-network) sees no command symbols and so doesn't accidentally
// expose a UI surface that has no engine to talk to.

#![cfg(feature = "p2p")]

use super::AppStateArg;
use crate::error::{AppError, Result};
use crate::p2p::pairing::{self, PairingPayload};
use crate::p2p::SyncEngine;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long the joiner waits for the seed's snapshot reply before
/// giving up. The QR is one-shot anyway — a hung handshake means
/// network failure or replay rejection, both of which the user needs
/// surfaced.
const PAIR_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(15);

/// Status payload returned by `p2p_status` / `p2p_enable` / `p2p_consume_pairing`
/// / `p2p_rotate`. Mirrors design.md §8.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct P2pStatus {
    /// `user_preferences.p2p_enabled` mirrored — true if the user
    /// has flipped the toggle on. Independent of whether the engine
    /// is currently running (e.g. it could be `true` but startup
    /// failed; the UI should surface that via the error event).
    pub enabled: bool,
    /// This device's iroh `EndpointId` in z32 (display) form. `None`
    /// when the engine isn't running.
    pub node_id: Option<String>,
    /// Other devices currently paired with this one, loaded from the
    /// durable paired_devices roster.
    pub paired_peers: Vec<PairedPeer>,
    /// RFC3339 timestamp of the most recent inbound apply, if any.
    /// `None` until the bridge logs an apply (T-022).
    pub last_apply_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedPeer {
    pub node_id: String,
    pub last_seen_at: Option<String>,
}

/// QR rendering payload returned by `p2p_pairing_qr`. The frontend
/// gets both the structured payload (for inspection / toast text)
/// and the rendered PNG (`<img src="data:image/png;base64,...">`)
/// so it doesn't need to ship a JS QR library.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingQr {
    pub payload: PairingPayload,
    /// Base64 PNG of the QR rendered from the payload's encoded form.
    pub png_b64: String,
    /// The raw base64-of-JSON string that was encoded into the QR.
    /// Useful when the joining device is on the same machine (manual
    /// paste flow) rather than scanning with a camera.
    pub encoded: String,
}

/// `consume_pairing` payload from the React side. We accept the
/// raw QR-decoded base64 string — that's what the camera scanner
/// hands us — and run parse + replay-check inside the command.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumePairingArgs {
    /// Base64-encoded JSON pairing payload (the literal QR data).
    pub encoded: String,
}

#[tauri::command]
pub async fn p2p_status(state: AppStateArg<'_>) -> Result<P2pStatus> {
    Ok(build_status(&state).await)
}

/// First call: persist `p2p_enabled = 1`, generate / load secrets,
/// spawn the engine. Subsequent calls: return current status (idempotent).
#[tauri::command]
pub async fn p2p_enable(state: AppStateArg<'_>) -> Result<P2pStatus> {
    set_enabled_flag(&state, true).await?;
    if state.sync_engine().is_none() {
        let engine = SyncEngine::start(state.inner().clone()).await?;
        *state.sync.write() = Some(engine);
    }
    Ok(build_status(&state).await)
}

/// Stop the engine. Leaves persisted secrets in place — re-enabling
/// resumes the same identity and the same gossip topic. Use
/// `p2p_rotate` to actually invalidate paired devices.
#[tauri::command]
pub async fn p2p_disable(state: AppStateArg<'_>) -> Result<()> {
    set_enabled_flag(&state, false).await?;
    let engine = state.sync.write().take();
    if let Some(engine) = engine {
        if let Err(e) = engine.shutdown().await {
            tracing::warn!("p2p engine shutdown: {e}");
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn p2p_pairing_qr(state: AppStateArg<'_>) -> Result<PairingQr> {
    let engine = state
        .sync_engine()
        .ok_or_else(|| AppError::BadRequest("p2p engine not running".into()))?;

    // Pull the current sync_doc_secret so it goes into the QR. The
    // engine has it in memory but not exposed; we read it back from
    // the same canonical store (keychain / app_state fallback) that
    // `load_or_init_secrets` populates. Reading on demand keeps the
    // QR honest if rotation has happened.
    let secret_hex = crate::p2p::secrets::load(
        &state.pool,
        crate::p2p::secrets::SecretKind::SyncDocSecret,
    )
    .await?
    .ok_or_else(|| AppError::Other("sync_doc_secret missing after p2p_enable".into()))?;

    let node = engine.endpoint_id().to_string();
    // `addrs` and `relay` are best-effort hints; the joiner already
    // has the EndpointId (from `node`) which is enough for iroh's
    // dial path. We leave them empty / `None` in v1 — discovery via
    // the n0 default DNS + relay path covers the rest.
    let payload = PairingPayload::new(node, Vec::new(), None, secret_hex);
    let png = pairing::generate_qr_png(&payload)?;
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    let png_b64 = B64.encode(&png);
    let encoded = B64.encode(payload.to_json()?.as_bytes());

    Ok(PairingQr {
        payload,
        png_b64,
        encoded,
    })
}

#[tauri::command]
pub async fn p2p_consume_pairing(
    state: AppStateArg<'_>,
    args: ConsumePairingArgs,
) -> Result<P2pStatus> {
    if state.sync_engine().is_some() {
        return Err(AppError::BadRequest(
            "engine already running — disable before pairing this device".into(),
        ));
    }

    // Validate + record the nonce locally. (The seed's
    // consumed_pairing_nonces is a separate DB; this one defends
    // against double-pair on the same joiner.)
    let payload = pairing::consume_payload(&state.pool, &args.encoded).await?;

    // Persist enabled flag BEFORE bringing up the engine, so a crash
    // mid-pairing still presents as "enabled, restart the app" rather
    // than silently rolling back.
    set_enabled_flag(&state, true).await?;

    let engine = SyncEngine::pair_via_payload(state.inner().clone(), &payload).await?;

    // Wait for the seed's snapshot reply; if it never comes, the
    // pairing failed (network / replay / wrong topic). Surface to
    // the user rather than silently leaving an empty engine running.
    if let Err(e) = engine.await_initial_snapshot(PAIR_SNAPSHOT_TIMEOUT).await {
        engine.shutdown().await.ok();
        return Err(AppError::Other(format!(
            "pairing handshake failed: {e}"
        )));
    }

    remember_paired_device(&state.pool, &payload.node, Some("Seed device")).await?;

    // Hydrate SQLite from the freshly-imported Loro doc. Composite-key
    // overlays whose Brightspace rows aren't fetched yet park into
    // pending_overlay_apply (T-011).
    if let Err(e) = engine.bridge().hydrate_from_doc().await {
        tracing::warn!("post-pairing hydrate failed: {e}");
    }

    *state.sync.write() = Some(engine);
    Ok(build_status(&state).await)
}

/// On-disk sync persistence numbers, surfaced in Settings → Sync.
/// Mirrors `SyncStore::storage_stats` 1:1; the command exists so
/// the React side can poll without holding a Tauri::State across
/// awaits.
#[tauri::command]
pub async fn p2p_storage_stats(
    state: AppStateArg<'_>,
) -> Result<crate::p2p::persistence::StorageStats> {
    let engine = state
        .sync_engine()
        .ok_or_else(|| AppError::BadRequest("p2p engine not running".into()))?;
    Ok(engine.store().storage_stats())
}

/// Generate a fresh `sync_doc_secret`, restart the engine. Old peers
/// remain on the previous gossip topic and see no new traffic; they
/// effectively drop off the sync group. The user has to re-pair every
/// other device after rotating.
#[tauri::command]
pub async fn p2p_rotate(state: AppStateArg<'_>) -> Result<P2pStatus> {
    // Tear down the running engine first; we need to release the
    // iroh endpoint before re-binding under the new topic. We take
    // the Arc out of the lock in its own scope so the parking_lot
    // guard (not Send) is dropped before we hit the await below.
    let prior = state.sync.write().take();
    if let Some(engine) = prior {
        if let Err(e) = engine.shutdown().await {
            tracing::warn!("rotate: prior engine shutdown: {e}");
        }
    }

    // Generate + persist a new shared secret via the same backend
    // (keychain primary, app_state fallback) that load_or_init_secrets
    // uses. Joiner-side re-pairing picks this up via a fresh QR.
    use rand::RngCore;
    let mut buf = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    let new_secret = hex::encode(&buf);
    crate::p2p::secrets::store(
        &state.pool,
        crate::p2p::secrets::SecretKind::SyncDocSecret,
        &new_secret,
    )
    .await?;

    // Wipe the consumed-nonces table — the old QRs are dead anyway,
    // and rotating effectively means "forget everyone, start over".
    sqlx::query("DELETE FROM consumed_pairing_nonces")
        .execute(&state.pool)
        .await?;

    // Rotation invalidates all prior peers; clear the roster so the UI
    // accurately communicates that every device must be re-paired.
    sqlx::query("DELETE FROM paired_devices")
        .execute(&state.pool)
        .await?;

    // Bring the engine back up under the new secret.
    let engine = SyncEngine::start(state.inner().clone()).await?;
    *state.sync.write() = Some(engine);

    Ok(build_status(&state).await)
}

// ---- helpers --------------------------------------------------------------

async fn set_enabled_flag(state: &AppStateArg<'_>, enabled: bool) -> Result<()> {
    sqlx::query(
        "UPDATE user_preferences SET p2p_enabled = ?, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(enabled as i64)
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn build_status(state: &AppStateArg<'_>) -> P2pStatus {
    let enabled: i64 = sqlx::query_scalar("SELECT p2p_enabled FROM user_preferences LIMIT 1")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
    let node_id = state.sync_engine().map(|e| e.endpoint_id().to_string());
    let paired_peers = load_paired_peers(&state.pool).await.unwrap_or_else(|e| {
        tracing::warn!("load paired_devices failed: {e}");
        Vec::new()
    });
    P2pStatus {
        enabled: enabled != 0,
        node_id,
        paired_peers,
        last_apply_at: None,
    }
}

async fn remember_paired_device(
    pool: &sqlx::SqlitePool,
    node_id: &str,
    label: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO paired_devices (id, public_key, label, last_seen_at) \
         VALUES (?, ?, ?, CURRENT_TIMESTAMP) \
         ON CONFLICT(id) DO UPDATE SET \
           public_key = excluded.public_key, \
           label = COALESCE(excluded.label, paired_devices.label), \
           last_seen_at = CURRENT_TIMESTAMP, \
           updated_at = CURRENT_TIMESTAMP",
    )
    .bind(node_id)
    .bind(node_id)
    .bind(label)
    .execute(pool)
    .await?;
    Ok(())
}

async fn load_paired_peers(pool: &sqlx::SqlitePool) -> Result<Vec<PairedPeer>> {
    let rows = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT id, last_seen_at FROM paired_devices ORDER BY last_seen_at DESC, created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(node_id, last_seen_at)| PairedPeer {
            node_id,
            last_seen_at,
        })
        .collect())
}
