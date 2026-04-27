// Database setup. Uses sqlx with a single SQLite file under the OS app-data dir.

use crate::error::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::str::FromStr;
use tauri::AppHandle;
use tauri::Manager;

pub async fn init(app: &AppHandle) -> Result<SqlitePool> {
    let path = data_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    tracing::info!("opening sqlite at {}", path.display());

    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new().max_connections(8).connect_with(opts).await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    // Ensure user_preferences row exists.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_preferences")
        .fetch_one(&pool)
        .await?;
    if count == 0 {
        sqlx::query("INSERT INTO user_preferences (api_port) VALUES (4567)")
            .execute(&pool)
            .await?;
    }

    Ok(pool)
}

fn data_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_data_dir().map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    Ok(dir.join("brilliant.sqlite3"))
}
