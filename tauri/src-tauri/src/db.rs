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

    run_migrations(&pool).await?;

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

/// Apply the embedded migrations. Split out from `init` so the ignore-missing
/// behaviour below is reachable from a test without standing up a Tauri app.
async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    // A database can legitimately record migrations this binary does not have:
    // installing a TestFlight build over a dev build (or rolling back a release)
    // leaves rows in `_sqlx_migrations` from the newer tree. sqlx treats that as
    // fatal by default — "migration N was previously applied but is missing in
    // the resolved migrations" — which is what took down every launch of 2.0.12
    // on devices that had run a dev build carrying a 20th migration. An unknown
    // *newer* migration is additive schema this build simply doesn't use; it is
    // not a reason to refuse to start.
    //
    // This does NOT relax checksum validation: a migration that was applied and
    // then *modified* still fails loudly, which is what the
    // `check-migration-numbers.sh` CI guard exists for.
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.set_ignore_missing(true);
    migrator.run(pool).await?;
    Ok(())
}

fn data_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_data_dir().map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    Ok(dir.join("brilliant.sqlite3"))
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    /// A database that records a migration this binary doesn't have is exactly
    /// what a device looks like after running a dev build and then installing a
    /// release one. It used to abort the app at launch (2.0.12 build 3, "migration
    /// 20 was previously applied but is missing in the resolved migrations"), so
    /// the ignore-missing behaviour is pinned here.
    #[tokio::test]
    async fn unknown_newer_migration_does_not_block_startup() {
        // One connection so the in-memory database survives between queries.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        super::run_migrations(&pool).await.unwrap();

        // Stand in for the migration the newer build applied.
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
             VALUES (9999, 'applied by a build this one does not know', CURRENT_TIMESTAMP, 1, X'00', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // What the next launch does.
        super::run_migrations(&pool)
            .await
            .expect("a migration from a newer build must not stop the app starting");
    }
}
