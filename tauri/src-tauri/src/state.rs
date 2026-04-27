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
}

impl AppState {
    pub async fn initialize(app: AppHandle) -> Result<Self> {
        let pool = db::init(&app).await?;
        let client = BrightspaceClient::from_db(&pool).await?;
        Ok(Self {
            pool,
            client: Arc::new(client),
            sync_status: Arc::new(RwLock::new(SyncStatus::default())),
            events: EventBus::new(app.clone()),
            rest_handle: Mutex::new(None),
            app,
        })
    }
}
