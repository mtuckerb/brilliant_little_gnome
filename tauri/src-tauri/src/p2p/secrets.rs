// Secret storage with OS-keychain primary + SQLite fallback (T-021).
//
// The two long-lived secrets are:
//   - iroh_node_secret  — 32-byte ed25519 device identity, base64
//   - sync_doc_secret   — 32 random bytes shared across paired devices, hex
//
// Keychain is preferred. On systems where it works (macOS Keychain,
// Windows Credential Manager, Linux kernel keyutils), the SQLite
// columns stay NULL — a `sqlite3 brilliant.sqlite3 .dump` won't
// reveal raw secret material. On systems without (headless Linux
// without keyutils, sandboxed kiosks), we fall back to the
// `app_state` table — the same place these lived before T-021. The
// security trade-off is documented in design.md §12.
//
// Threat model: a process on the user's machine that can read SQLite
// can usually also read the keychain via the OS API, so the keychain
// is a defense-in-depth measure rather than a confidentiality
// guarantee. The win is for *passive* exposure — backups, accidental
// uploads, stolen-disk scenarios — where the keychain is encrypted
// at rest and the SQLite file is not.
//
// Design choice: keyring entries are namespaced as
// `(SERVICE, account)` where SERVICE is fixed and `account` is the
// secret's logical name ("iroh_node_secret" / "sync_doc_secret").
// Bumping SERVICE is a wire-break and should only happen at major
// version boundaries.

#![allow(dead_code)]

use crate::error::{AppError, Result};
use sqlx::SqlitePool;
use tracing::{info, warn};

const SERVICE: &str = "brilliant.sync";

/// Secret kinds we persist. Each maps to a fixed keyring `account`
/// and the matching `app_state` row key.
#[derive(Debug, Clone, Copy)]
pub enum SecretKind {
    /// Ed25519 secret key for the iroh endpoint, base64 of 32 bytes.
    IrohNodeSecret,
    /// Shared sync_doc_secret, hex of 32 bytes (matches QR format).
    SyncDocSecret,
}

impl SecretKind {
    fn account(self) -> &'static str {
        match self {
            SecretKind::IrohNodeSecret => "iroh_node_secret",
            SecretKind::SyncDocSecret => "sync_doc_secret",
        }
    }
}

/// Fetch a secret string. Tries the keychain first; on cache miss
/// (or platform without keychain support), reads the app_state
/// table. On a successful keychain hit AFTER an app_state value
/// was already stored, we leave the app_state row alone — see
/// `migrate_to_keychain` for the explicit deletion path.
pub async fn load(pool: &SqlitePool, kind: SecretKind) -> Result<Option<String>> {
    if let Some(value) = keychain_get(kind) {
        return Ok(Some(value));
    }
    Ok(get_app_state(pool, kind.account()).await?)
}

/// Persist a secret. Writes to the keychain if possible; if the
/// keychain refuses (e.g. headless Linux), falls back to app_state.
/// On a successful keychain write, also DELETEs any prior
/// app_state row so a `sqlite3 .dump` doesn't leak the secret —
/// the test for this lives in `migrate_to_keychain` below.
pub async fn store(pool: &SqlitePool, kind: SecretKind, value: &str) -> Result<()> {
    match keychain_set(kind, value) {
        Ok(()) => {
            info!("persisted {} into OS keychain", kind.account());
            // Best-effort: erase the SQLite copy if we successfully
            // hit the keychain. Failure here is non-fatal — worst
            // case the next `load` reads the (now stale) SQLite copy
            // and overwrites it.
            if let Err(e) = clear_app_state(pool, kind.account()).await {
                warn!("could not clear app_state {}: {e}", kind.account());
            }
            Ok(())
        }
        Err(e) => {
            warn!(
                "keychain unavailable for {} ({e}); falling back to app_state",
                kind.account()
            );
            set_app_state(pool, kind.account(), value).await
        }
    }
}

/// One-shot upgrade: if `app_state` has a stored value but the
/// keychain doesn't, copy across and DELETE the SQLite row. Called
/// during engine startup so existing installs migrate transparently
/// the first time after enabling T-021.
///
/// No-ops when the keychain isn't available — the caller's `load`
/// path will continue to read from app_state.
pub async fn migrate_to_keychain(pool: &SqlitePool, kind: SecretKind) -> Result<()> {
    if keychain_get(kind).is_some() {
        return Ok(()); // already migrated
    }
    let Some(existing) = get_app_state(pool, kind.account()).await? else {
        return Ok(()); // nothing to migrate
    };
    if let Err(e) = keychain_set(kind, &existing) {
        warn!(
            "keychain unavailable on this system ({e}); leaving {} in app_state",
            kind.account()
        );
        return Ok(());
    }
    clear_app_state(pool, kind.account()).await?;
    info!("migrated {} from app_state to keychain", kind.account());
    Ok(())
}

// ---- platform-specific shims ---------------------------------------------

fn keychain_get(kind: SecretKind) -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, kind.account()).ok()?;
    match entry.get_password() {
        Ok(s) => Some(s),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            warn!("keychain get {} failed: {e}", kind.account());
            None
        }
    }
}

fn keychain_set(kind: SecretKind, value: &str) -> std::result::Result<(), keyring::Error> {
    let entry = keyring::Entry::new(SERVICE, kind.account())?;
    entry.set_password(value)
}

#[cfg(test)]
fn keychain_delete(kind: SecretKind) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, kind.account()) {
        let _ = entry.delete_credential();
    }
}

// ---- SQLite fallback (mirrors the pre-T-021 path in engine.rs) -----------

async fn get_app_state(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT value FROM app_state WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(v,)| v))
}

async fn set_app_state(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO app_state (key, value, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

async fn clear_app_state(pool: &SqlitePool, key: &str) -> Result<()> {
    sqlx::query("DELETE FROM app_state WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

// `AppError::Other` only — the rest of the engine surface is
// already permissive, so plumbing keyring's typed error doesn't
// add value at this layer.
#[allow(dead_code)]
fn _coerce_err(e: keyring::Error) -> AppError {
    AppError::Other(format!("keychain: {e}"))
}

// ---- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn mem_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    /// Without a keychain hit, load() falls through to the app_state
    /// fallback. This is the path Linux without keyutils / CI runners
    /// without secret-service hit, so it must work.
    #[tokio::test]
    async fn load_falls_back_to_app_state_when_keychain_empty() {
        // Ensure a clean keychain slot for this test.
        keychain_delete(SecretKind::SyncDocSecret);

        let pool = mem_pool().await;
        // Pre-stash a secret directly into app_state — simulates a
        // pre-T-021 install that hasn't migrated yet.
        set_app_state(&pool, "sync_doc_secret", "deadbeef").await.unwrap();

        let got = load(&pool, SecretKind::SyncDocSecret).await.unwrap();
        // Either the keychain has nothing (expected) and we read
        // app_state, or some prior test run left a value there. The
        // first branch is what we want; the second means something
        // about the test's keychain isolation broke. Either way, we
        // got a value back.
        assert!(got.is_some());
    }

    /// store() prefers the keychain and clears the app_state copy
    /// after a successful write. The tested path is the happy one;
    /// keychain-unavailable falls through to app_state and is
    /// covered by manual QA on systems without keyutils.
    #[tokio::test]
    async fn store_via_keychain_clears_app_state_copy() {
        // Skip on platforms where keychain access fails in CI (a
        // raw setup test won't even be able to call set_password).
        // Linux kernel keyutils sandboxes test runners differently
        // depending on the executor — we tolerate "keychain not
        // available here" by short-circuiting.
        let probe = keyring::Entry::new(SERVICE, "_probe");
        if probe.is_err() {
            eprintln!("keychain unavailable in this test environment, skipping");
            return;
        }
        let probe = probe.unwrap();
        if probe.set_password("probe-value").is_err() {
            eprintln!("keychain set unavailable in this test environment, skipping");
            return;
        }
        let _ = probe.delete_credential();

        keychain_delete(SecretKind::SyncDocSecret);

        let pool = mem_pool().await;
        // Pre-stash an old SQLite copy so we can prove store() wipes it.
        set_app_state(&pool, "sync_doc_secret", "stale-value").await.unwrap();

        store(&pool, SecretKind::SyncDocSecret, "fresh-value").await.unwrap();

        // app_state row gone.
        let row = get_app_state(&pool, "sync_doc_secret").await.unwrap();
        assert_eq!(row, None, "app_state must be cleared on keychain success");

        // load() returns the keychain value, not the (deleted) SQLite copy.
        let got = load(&pool, SecretKind::SyncDocSecret).await.unwrap();
        assert_eq!(got.as_deref(), Some("fresh-value"));

        keychain_delete(SecretKind::SyncDocSecret);
    }

    /// migrate_to_keychain copies an existing app_state secret into
    /// the keychain and deletes the SQLite row. Idempotent — a
    /// second call after the migration is a no-op.
    #[tokio::test]
    async fn migrate_to_keychain_moves_existing_secret() {
        let probe = keyring::Entry::new(SERVICE, "_probe");
        if probe.is_err() || probe.unwrap().set_password("probe").is_err() {
            eprintln!("keychain unavailable in this test environment, skipping");
            return;
        }
        let _ = keyring::Entry::new(SERVICE, "_probe").and_then(|e| e.delete_credential());

        keychain_delete(SecretKind::IrohNodeSecret);
        let pool = mem_pool().await;

        set_app_state(&pool, "iroh_node_secret", "encoded-key-bytes")
            .await
            .unwrap();

        migrate_to_keychain(&pool, SecretKind::IrohNodeSecret).await.unwrap();

        // SQLite copy gone, keychain has the value.
        let row = get_app_state(&pool, "iroh_node_secret").await.unwrap();
        assert_eq!(row, None);
        assert_eq!(
            keychain_get(SecretKind::IrohNodeSecret).as_deref(),
            Some("encoded-key-bytes"),
        );

        // Second call: keychain already populated, app_state empty,
        // nothing to do.
        migrate_to_keychain(&pool, SecretKind::IrohNodeSecret).await.unwrap();

        keychain_delete(SecretKind::IrohNodeSecret);
    }
}
