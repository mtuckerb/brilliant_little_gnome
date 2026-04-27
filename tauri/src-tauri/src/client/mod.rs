// Brightspace HTTP client. Stub — full endpoint coverage lands in a follow-up.
// Loads cookie + host from the DB, exposes a reqwest client wired with a cookie jar
// so subsequent calls behave like an authenticated browser session.

use crate::error::{AppError, Result};
use parking_lot::RwLock;
use reqwest::Client;
use sqlx::SqlitePool;
use std::sync::Arc;

pub struct BrightspaceClient {
    pub host: RwLock<Option<String>>,
    pub cookie: RwLock<Option<String>>,
    pub uid: RwLock<Option<String>>,
    pub user_id: RwLock<Option<String>>,
    pub http: Client,
}

impl BrightspaceClient {
    pub async fn from_db(pool: &SqlitePool) -> Result<Self> {
        let row: Option<(Option<String>, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT brightspace_host, brightspace_cookie, brightspace_uid, brightspace_user_id FROM user_preferences LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;
        let (host, cookie, uid, user_id) = row.unwrap_or((None, None, None, None));

        let http = Client::builder()
            .cookie_store(true)
            .user_agent("brilliant-tauri/0.1")
            .build()?;

        Ok(Self {
            host: RwLock::new(host),
            cookie: RwLock::new(cookie),
            uid: RwLock::new(uid),
            user_id: RwLock::new(user_id),
            http,
        })
    }

    pub fn is_configured(&self) -> bool {
        self.host.read().is_some() && self.cookie.read().is_some()
    }

    pub async fn store_credentials(
        &self,
        pool: &SqlitePool,
        host: &str,
        cookie: &str,
        uid: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE user_preferences SET brightspace_host = ?, brightspace_cookie = ?, brightspace_uid = ?, brightspace_user_id = ?, last_login_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT id FROM user_preferences LIMIT 1)",
        )
        .bind(host)
        .bind(cookie)
        .bind(uid)
        .bind(user_id)
        .execute(pool)
        .await?;
        *self.host.write() = Some(host.to_string());
        *self.cookie.write() = Some(cookie.to_string());
        *self.uid.write() = uid.map(|s| s.to_string());
        *self.user_id.write() = user_id.map(|s| s.to_string());
        Ok(())
    }

    pub async fn clear_credentials(&self, pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            "UPDATE user_preferences SET brightspace_cookie = NULL, brightspace_uid = NULL, brightspace_user_id = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT id FROM user_preferences LIMIT 1)",
        )
        .execute(pool)
        .await?;
        *self.cookie.write() = None;
        *self.uid.write() = None;
        *self.user_id.write() = None;
        Ok(())
    }

    pub fn host_clone(&self) -> Option<String> { self.host.read().clone() }
    pub fn cookie_clone(&self) -> Option<String> { self.cookie.read().clone() }
    pub fn uid_clone(&self) -> Option<String> { self.uid.read().clone() }
    pub fn user_id_clone(&self) -> Option<String> { self.user_id.read().clone() }

    /// Placeholder — real implementation fetches and parses paginated JSON with
    /// bookmark cursors, like the Ruby client's `get_paginated`.
    #[allow(dead_code)]
    pub async fn get_paginated(&self, _path: &str) -> Result<Vec<serde_json::Value>> {
        Err(AppError::Other("BrightspaceClient::get_paginated not yet implemented in port".into()))
    }
}

pub type SharedClient = Arc<BrightspaceClient>;
