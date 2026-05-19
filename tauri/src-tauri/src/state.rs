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
    /// Best-effort: any failure (no sync engine running, transient Loro
    /// error) is logged and swallowed. The credentials are already saved
    /// locally before this is called.
    #[cfg(feature = "p2p")]
    pub async fn mirror_credentials_to_loro(&self) {
        let Some(engine) = self.sync_engine() else { return };
        use crate::p2p::bridge::{LocalChange, PrefField};
        let bridge = engine.bridge();
        if let Some(cookie) = self.client.cookie_clone() {
            if let Err(e) = bridge
                .apply_local(LocalChange::Pref(PrefField::BrightspaceCookie(cookie)))
                .await
            {
                tracing::warn!("apply_local brightspace_cookie: {e}");
            }
        }
        if let Some(host) = self.client.host_clone() {
            if let Err(e) = bridge
                .apply_local(LocalChange::Pref(PrefField::BrightspaceHost(host)))
                .await
            {
                tracing::warn!("apply_local brightspace_host: {e}");
            }
        }
    }
}
