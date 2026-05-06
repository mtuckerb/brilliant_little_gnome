// QR-code pairing flow. Design.md §6.
//
// T-012 implements the QR payload + parser + nonce table.
// T-013 will implement the actual handshake protocol over the transport.
//
// ---- Validation contract -------------------------------------------------
//
// `parse_payload` is pure: it decodes the JSON, checks the protocol
// version, and rejects anything past its `exp` timestamp. It does NOT
// touch the database — the replay-detection step (`consume_nonce`) is
// async and lives separately so callers can place the side effect at
// the right boundary (e.g. a Tauri command).
//
// `consume_nonce` does an `INSERT … ON CONFLICT FAIL` against
// `consumed_pairing_nonces`. The first call for a given nonce
// succeeds; every subsequent call returns `AppError::BadRequest` so a
// screenshot replay reaches no further than this gate. The seed
// device records the nonce when it accepts the joining peer's
// state-request — see T-013 for the wire-level wiring.
//
// `consume_payload` is the convenience that pairs both: parse + replay
// check in one call. Production code should always go through this so
// the validity gate is auditable in a single place.

#![allow(dead_code)]

use crate::error::{AppError, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::Utc;
use image::{ImageBuffer, Luma};
use qrcode::QrCode;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::io::Cursor;

/// Wire-protocol version for the pairing payload. Bump on any
/// breaking change; `parse_payload` rejects anything ≠ this.
pub const PAIRING_VERSION: u8 = 1;

/// Time-to-live for a pairing QR, in seconds. The QR is meant to be
/// scanned within seconds — 60s gives the user a comfortable window
/// without leaving a long replay surface if the screenshot leaks.
pub const PAIRING_TTL_SECS: i64 = 60;

/// QR rendering: pixel size of the rendered PNG. 512×512 is the size
/// the React UI scales into a phone-camera-friendly box.
const QR_PIXELS: u32 = 512;

/// JSON-serialised payload encoded into the pairing QR code. The fields
/// match design.md §6: NodeId in z32, candidate addrs (best-effort dialing
/// hint), optional relay URL, the shared sync_doc_secret, a one-shot
/// nonce, and an absolute expiry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingPayload {
    pub v: u8,                 // == PAIRING_VERSION
    pub node: String,          // NodeId in z32
    pub addrs: Vec<String>,    // direct addrs (best-effort hint)
    pub relay: Option<String>, // relay url
    pub secret: String,        // sync_doc_secret hex
    pub nonce: String,         // 16 random bytes hex; one-shot
    pub exp: i64,              // unix ts; PAIRING_TTL_SECS in the future
}

impl PairingPayload {
    /// Build a fresh payload for the seed device to display. Generates
    /// a random 16-byte nonce, sets `exp` to `now + PAIRING_TTL_SECS`.
    pub fn new(
        node: String,
        addrs: Vec<String>,
        relay: Option<String>,
        secret_hex: String,
    ) -> Self {
        let mut nonce_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = hex::encode(nonce_bytes);
        let exp = Utc::now().timestamp() + PAIRING_TTL_SECS;
        Self {
            v: PAIRING_VERSION,
            node,
            addrs,
            relay,
            secret: secret_hex,
            nonce,
            exp,
        }
    }

    /// Serialise to compact JSON (no whitespace).
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(AppError::from)
    }
}

/// Render a pairing QR as a 512×512 grayscale PNG.
///
/// The QR data is the base64 of the JSON payload. base64 keeps the
/// payload in QR's "alphanumeric" range and stays compact (~340 chars
/// for a typical seed payload, well within QR version 10/M capacity).
pub fn generate_qr_png(payload: &PairingPayload) -> Result<Vec<u8>> {
    let json = payload.to_json()?;
    let encoded = B64.encode(json.as_bytes());
    let code = QrCode::new(encoded.as_bytes())
        .map_err(|e| AppError::Other(format!("qrcode encode: {e}")))?;
    let image: ImageBuffer<Luma<u8>, Vec<u8>> = code
        .render::<Luma<u8>>()
        .min_dimensions(QR_PIXELS, QR_PIXELS)
        .max_dimensions(QR_PIXELS, QR_PIXELS)
        .build();

    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    image
        .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| AppError::Other(format!("png encode: {e}")))?;
    Ok(buf)
}

/// Parse a base64-encoded JSON pairing payload (the inverse of what
/// went into the QR). Validates the version and the absolute expiry.
///
/// Does NOT check the nonce against the database — see
/// `consume_nonce` / `consume_payload`. Splitting the side-effecting
/// step keeps this function pure for tests.
pub fn parse_payload(encoded: &str) -> Result<PairingPayload> {
    let bytes = B64
        .decode(encoded.trim().as_bytes())
        .map_err(|e| AppError::BadRequest(format!("pairing payload base64: {e}")))?;
    let payload: PairingPayload = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::BadRequest(format!("pairing payload json: {e}")))?;

    if payload.v != PAIRING_VERSION {
        return Err(AppError::BadRequest(format!(
            "pairing payload version {} unsupported (want {})",
            payload.v, PAIRING_VERSION,
        )));
    }
    let now = Utc::now().timestamp();
    if payload.exp <= now {
        return Err(AppError::BadRequest(format!(
            "pairing payload expired ({}s ago)",
            now - payload.exp
        )));
    }
    Ok(payload)
}

/// Insert `nonce` into `consumed_pairing_nonces` atomically. Returns
/// `Ok(())` on first use, `BadRequest` on replay. Use this on the
/// receiving (seed) side once the joiner's state-request arrives, so
/// the QR can't be reused even within its expiry window.
pub async fn consume_nonce(pool: &SqlitePool, nonce: &str) -> Result<()> {
    let res = sqlx::query(
        "INSERT INTO consumed_pairing_nonces (nonce) VALUES (?) \
         ON CONFLICT(nonce) DO NOTHING",
    )
    .bind(nonce)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::BadRequest(format!(
            "pairing nonce {nonce} already consumed"
        )));
    }
    Ok(())
}

/// Convenience: parse + replay-check in one call. Production callers
/// (the `p2p_consume_pairing` Tauri command in T-014) should use this
/// so the full validity gate is one auditable step.
pub async fn consume_payload(pool: &SqlitePool, encoded: &str) -> Result<PairingPayload> {
    let payload = parse_payload(encoded)?;
    consume_nonce(pool, &payload.nonce).await?;
    Ok(payload)
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as B64;
    use sqlx::sqlite::SqlitePoolOptions;

    fn sample_payload() -> PairingPayload {
        PairingPayload::new(
            "k7n3jd6mz...node...z32xyz".into(),
            vec!["192.168.1.5:11204".into(), "[fe80::1]:11204".into()],
            Some("https://relay.iroh.network".into()),
            "deadbeef".repeat(8),
        )
    }

    fn encode(payload: &PairingPayload) -> String {
        B64.encode(payload.to_json().unwrap().as_bytes())
    }

    async fn mem_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[test]
    fn payload_round_trips_through_json() {
        let p = sample_payload();
        let json = p.to_json().unwrap();
        let p2: PairingPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn new_payload_has_unique_nonce_and_future_expiry() {
        let a = sample_payload();
        let b = sample_payload();
        assert_ne!(a.nonce, b.nonce, "nonces must be unique per QR");
        assert_eq!(a.nonce.len(), 32, "16 bytes hex = 32 chars");
        assert!(a.exp > Utc::now().timestamp(), "exp must be in the future");
        assert!(
            a.exp - Utc::now().timestamp() <= PAIRING_TTL_SECS,
            "exp must be within TTL",
        );
    }

    #[test]
    fn generate_qr_png_emits_valid_png_header() {
        let p = sample_payload();
        let png = generate_qr_png(&p).unwrap();
        // PNG magic: \x89 P N G \r \n \x1a \n
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(png.len() > 1000, "rendered PNG should be non-trivial");
    }

    #[test]
    fn parse_payload_round_trip() {
        let p = sample_payload();
        let parsed = parse_payload(&encode(&p)).unwrap();
        assert_eq!(parsed, p);
    }

    #[test]
    fn parse_payload_rejects_bad_base64() {
        let err = parse_payload("not-valid-base64!@#").unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn parse_payload_rejects_bad_json() {
        let bogus = B64.encode(b"not json at all {{{");
        let err = parse_payload(&bogus).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn parse_payload_rejects_wrong_version() {
        let mut p = sample_payload();
        p.v = 2;
        let err = parse_payload(&encode(&p)).unwrap_err();
        assert!(
            matches!(&err, AppError::BadRequest(msg) if msg.contains("version")),
            "got {err:?}",
        );
    }

    #[test]
    fn parse_payload_rejects_expired() {
        let mut p = sample_payload();
        p.exp = Utc::now().timestamp() - 1; // exp in the past
        let err = parse_payload(&encode(&p)).unwrap_err();
        assert!(
            matches!(&err, AppError::BadRequest(msg) if msg.contains("expired")),
            "got {err:?}",
        );
    }

    #[tokio::test]
    async fn consume_nonce_succeeds_once_then_rejects_replay() {
        let pool = mem_pool().await;
        consume_nonce(&pool, "abc123").await.unwrap();
        let err = consume_nonce(&pool, "abc123").await.unwrap_err();
        assert!(
            matches!(&err, AppError::BadRequest(msg) if msg.contains("already consumed")),
            "got {err:?}",
        );
    }

    #[tokio::test]
    async fn consume_nonce_independent_keys_dont_interfere() {
        let pool = mem_pool().await;
        consume_nonce(&pool, "first").await.unwrap();
        consume_nonce(&pool, "second").await.unwrap();
        consume_nonce(&pool, "third").await.unwrap();
        // Each replay rejects independently.
        consume_nonce(&pool, "second").await.unwrap_err();
    }

    #[tokio::test]
    async fn consume_payload_full_pipeline_first_use_succeeds() {
        let pool = mem_pool().await;
        let p = sample_payload();
        let got = consume_payload(&pool, &encode(&p)).await.unwrap();
        assert_eq!(got, p);
    }

    #[tokio::test]
    async fn consume_payload_rejects_replay() {
        let pool = mem_pool().await;
        let p = sample_payload();
        let enc = encode(&p);
        consume_payload(&pool, &enc).await.unwrap();
        let err = consume_payload(&pool, &enc).await.unwrap_err();
        assert!(
            matches!(&err, AppError::BadRequest(msg) if msg.contains("already consumed")),
            "got {err:?}",
        );
    }

    #[tokio::test]
    async fn consume_payload_rejects_expired_before_db_write() {
        let pool = mem_pool().await;
        let mut p = sample_payload();
        p.exp = Utc::now().timestamp() - 1;
        let err = consume_payload(&pool, &encode(&p)).await.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));

        // Crucially: an expired-payload rejection must NOT have written
        // the nonce to the table — otherwise a malicious peer could
        // permanently DOS the *real* future use of that nonce by replaying
        // an expired version. parse_payload runs first, so the DB write
        // never happens.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM consumed_pairing_nonces")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0, "expired payload must not record nonce");
    }

    #[tokio::test]
    async fn qr_round_trips_through_decode() {
        // Sanity: PNG encode is one-way for testing, but base64-of-JSON
        // (the QR _payload_ itself) round-trips. This is what a real
        // scanner would feed into parse_payload.
        let p = sample_payload();
        let enc = encode(&p);
        let pool = mem_pool().await;
        let parsed = consume_payload(&pool, &enc).await.unwrap();
        assert_eq!(parsed, p);
    }
}
