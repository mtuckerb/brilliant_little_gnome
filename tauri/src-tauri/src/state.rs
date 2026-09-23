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
    /// Serializes automatic credential recovery. A sync fans out many HTTP
    /// requests, and several can discover the same expiry at once; only one
    /// ordered walk of the peer roster should run.
    #[cfg(feature = "p2p")]
    auth_recovery: Mutex<()>,
}

impl AppState {
    pub async fn initialize(app: AppHandle) -> Result<Self> {
        // Both steps are tagged because this runs in the launch path: when it
        // fails on a user's device the message is all we get back (see
        // `startup.rs`), and "db::init: ..." vs "client init: ..." is the
        // difference between looking at SQLite and looking at TLS.
        let pool = db::init(&app)
            .await
            .map_err(|e| crate::error::AppError::Other(format!("db::init: {e}")))?;
        let client = BrightspaceClient::from_db(&pool, app.clone())
            .await
            .map_err(|e| crate::error::AppError::Other(format!("client init: {e}")))?;
        Ok(Self {
            pool,
            client: Arc::new(client),
            sync_status: Arc::new(RwLock::new(SyncStatus::default())),
            events: EventBus::new(app.clone()),
            rest_handle: Mutex::new(None),
            app,
            #[cfg(feature = "p2p")]
            sync: RwLock::new(None),
            #[cfg(feature = "p2p")]
            auth_recovery: Mutex::new(()),
        })
    }

    /// Snapshot the current sync engine, if any. Returns a clone of
    /// the inner Arc so the caller doesn't hold the lock while
    /// awaiting on the engine.
    #[cfg(feature = "p2p")]
    pub fn sync_engine(&self) -> Option<Arc<crate::p2p::SyncEngine>> {
        self.sync.read().clone()
    }

    /// Validate credentials that have just been stored while device sync is
    /// enabled. Cookies are deliberately not put in the replicated Loro doc;
    /// peers receive them only through the targeted request/response recovery
    /// protocol when their own session is invalid.
    #[cfg(feature = "p2p")]
    pub async fn validate_credentials_for_peer_recovery(&self) -> Result<()> {
        if self.sync_engine().is_none() {
            return Ok(());
        }
        use crate::client::SessionValidation;
        use crate::error::AppError;
        let (Some(cookie), Some(host)) = (self.client.cookie_clone(), self.client.host_clone())
        else {
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

        Ok(())
    }

    /// Recover from an invalid local Brightspace session by asking saved peers
    /// one at a time. The engine validates every response before persistence.
    /// Returns false when sync is disabled, the roster is empty, or no peer has
    /// a usable session.
    #[cfg(feature = "p2p")]
    pub async fn recover_credentials_from_peers(&self, rejected_cookie: &str) -> bool {
        let _guard = self.auth_recovery.lock().await;

        // Another request may have completed recovery while this caller was
        // waiting for the lock. Only re-probe when the cookie actually
        // changed; the caller already proved `rejected_cookie` invalid.
        if let (Some(host), Some(cookie)) = (self.client.host_clone(), self.client.cookie_clone()) {
            if cookie != rejected_cookie
                && self.client.validate_session(&host, &cookie).await
                    == crate::client::SessionValidation::Valid
            {
                self.client.mark_auth_healthy();
                return true;
            }
        }

        let Some(engine) = self.sync_engine() else {
            return false;
        };
        match engine
            .recover_credentials_from_peers(&self.pool, self.client.as_ref())
            .await
        {
            Ok(recovered) => recovered,
            Err(e) => {
                tracing::warn!("peer credential recovery failed: {e}");
                false
            }
        }
    }

    /// Persist a rotated Brightspace cookie so it is ready for a future peer
    /// request.
    ///
    /// Called at the end of a successful sync. `client.absorb_rotated_cookie`
    /// keeps the in-memory cookie current as Brightspace reissues the session
    /// during normal API calls; here we notice when that live value has
    /// diverged from what's stored and write it back so it survives restart
    /// and is what the P2P responder hands an expired peer. No-ops when nothing
    /// rotated.
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
            return; // nothing rotated since the last persistence
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
        tracing::info!("Brightspace session cookie rotated — persisted for peer recovery");
        #[cfg(feature = "p2p")]
        if let Err(e) = self.validate_credentials_for_peer_recovery().await {
            tracing::warn!("refresh_shared_credentials: validation failed: {e}");
        }
    }
}
