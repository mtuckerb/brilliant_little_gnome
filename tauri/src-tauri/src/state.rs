// Shared application state. Holds the DB pool, Brightspace client, sync status, REST handle.

use crate::client::BrightspaceClient;
use crate::db;
use crate::error::Result;
use crate::events::EventBus;
use crate::models::SyncStatus;
use parking_lot::RwLock;
use sqlx::SqlitePool;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;

pub struct AppState {
    pub pool: SqlitePool,
    pub client: Arc<BrightspaceClient>,
    pub sync_status: Arc<RwLock<SyncStatus>>,
    pub events: EventBus,
    pub rest_handle: Mutex<Option<crate::rest_api::RestHandle>>,
    pub app: AppHandle,

    // P2P device-to-device sync engine. `None` until the user opts in
    // via Settings → Sync (which calls the `p2p_enable` Tauri command,
    // added in T-014). Wrapped in an `RwLock` so the enable / disable
    // / rotate commands can swap it without locking the whole
    // `AppState`. Holders of the inner `Arc<SyncEngine>` keep the
    // engine alive for the duration of their borrow.
    #[cfg(feature = "p2p")]
    pub sync: RwLock<Option<Arc<crate::p2p::SyncEngine>>>,
}

impl AppState {
    pub async fn initialize(app: AppHandle) -> Result<Self> {
        let pool = db::init(&app).await?;
        let client = BrightspaceClient::from_db(&pool, app.clone()).await?;
        Ok(Self {
            pool,
            client: Arc::new(client),
            sync_status: Arc::new(RwLock::new(SyncStatus::default())),
            events: EventBus::new(app.clone()),
            rest_handle: Mutex::new(None),
            app,
            #[cfg(feature = "p2p")]
            sync: RwLock::new(None),
        })
    }

    /// Snapshot the current sync engine, if any. Returns a clone of
    /// the inner Arc so the caller doesn't hold the lock while
    /// awaiting on the engine.
    #[cfg(feature = "p2p")]
    pub fn sync_engine(&self) -> Option<Arc<crate::p2p::SyncEngine>> {
        self.sync.read().clone()
    }

    /// Push the current Brightspace cookie + host into the Loro doc so
    /// paired peers pick up the fresh session. Called after every
    /// `store_credentials` so the aggregate session lifetime across the
    /// device fleet stays as long as possible — when one device's auth
    /// expires, another peer's session takes over silently.
    ///
    /// The credentials are already saved locally before this is called;
    /// this only governs whether they are *shared* to peers.
    ///
    /// Pre-share validation gate: the Brightspace key is probed against the
    /// known-good auth endpoint first, and is published to the Loro doc only
    /// when confirmed live (`Valid`). An expired/incorrect key (`Invalid`)
    /// or an unverifiable one (`Inconclusive` — endpoint unreachable or
    /// misconfigured) fails closed and returns an actionable, key-free
    /// error instead of mirroring. Returns `Ok(())` (no-op) when sync is
    /// not enabled or there is nothing to share.
    #[cfg(feature = "p2p")]
    pub async fn mirror_credentials_to_loro(&self) -> Result<()> {
        let Some(engine) = self.sync_engine() else { return Ok(()) };
        use crate::client::SessionValidation;
        use crate::error::AppError;
        use crate::p2p::bridge::{LocalChange, PrefField};

        let (Some(cookie), Some(host)) = (self.client.cookie_clone(), self.client.host_clone())
        else {
            // No key/host to share.
            return Ok(());
        };

        match self.client.validate_session(&host, &cookie).await {
            SessionValidation::Valid => {}
            SessionValidation::Invalid => {
                return Err(AppError::BadRequest(
                    crate::client::SHARE_BLOCKED_INVALID.into(),
                ));
            }
            SessionValidation::Inconclusive => {
                return Err(AppError::Other(
                    crate::client::SHARE_BLOCKED_INCONCLUSIVE.into(),
                ));
            }
        }

        let bridge = engine.bridge();
        bridge
            .apply_local(LocalChange::Pref(PrefField::BrightspaceCookie(cookie)))
            .await
            .map_err(|e| AppError::Other(format!("apply_local brightspace_cookie: {e}")))?;
        bridge
            .apply_local(LocalChange::Pref(PrefField::BrightspaceHost(host)))
            .await
            .map_err(|e| AppError::Other(format!("apply_local brightspace_host: {e}")))?;
        Ok(())
    }

    /// Persist a rotated Brightspace cookie and re-share it to paired peers.
    ///
    /// Called at the end of a successful sync. `client.absorb_rotated_cookie`
    /// keeps the in-memory cookie current as Brightspace reissues the session
    /// during normal API calls; here we notice when that live value has
    /// diverged from what's stored, write it back (so it survives restart and
    /// is what the P2P bootstrap hands new peers), and re-mirror it into the
    /// Loro doc. The mirror runs the live-session validation gate and fails
    /// closed, so a bad value is never shared. The net effect: a paired phone
    /// stays authenticated as the desktop's session renews, with no manual
    /// re-login. No-ops when nothing rotated.
    pub async fn refresh_shared_credentials(&self) {
        let (Some(cookie), Some(host)) = (self.client.cookie_clone(), self.client.host_clone())
        else {
            return;
        };
        let stored: Option<String> =
            sqlx::query_scalar("SELECT brightspace_cookie FROM user_preferences LIMIT 1")
                .fetch_optional(&self.pool)
                .await
                .ok()
                .flatten();
        if stored.as_deref() == Some(cookie.as_str()) {
            return; // nothing rotated since the last share
        }
        if let Err(e) = sqlx::query(
            "UPDATE user_preferences SET brightspace_cookie = ?, brightspace_host = ?, updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT id FROM user_preferences LIMIT 1)",
        )
        .bind(&cookie)
        .bind(&host)
        .execute(&self.pool)
        .await
        {
            tracing::warn!("refresh_shared_credentials: persist failed: {e}");
            return;
        }
        tracing::info!("Brightspace session cookie rotated — persisted and re-sharing to peers");
        #[cfg(feature = "p2p")]
        if let Err(e) = self.mirror_credentials_to_loro().await {
            tracing::warn!("refresh_shared_credentials: mirror failed: {e}");
        }
    }
}
