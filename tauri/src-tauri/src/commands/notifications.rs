use super::AppStateArg;
use crate::error::Result;
use crate::models::Notification;
use sqlx::SqlitePool;

#[tauri::command]
pub async fn list_notifications(
    state: AppStateArg<'_>,
    unread_only: Option<bool>,
    course_id: Option<String>,
) -> Result<Vec<Notification>> {
    list_notifications_in(&state.pool, unread_only.unwrap_or(false), course_id.as_deref()).await
}

/// Pool-only helper so the SQL paths can be exercised in tests without
/// constructing an AppState or Tauri runtime.
pub async fn list_notifications_in(
    pool: &SqlitePool,
    unread_only: bool,
    course_id: Option<&str>,
) -> Result<Vec<Notification>> {
    // Course-scoped queries (used by the per-course Announcements view)
    // skip the LIMIT — there usually aren't that many per course, and the
    // user explicitly navigated there to see them.
    let rows = match (course_id, unread_only) {
        (Some(cid), true) => sqlx::query_as::<_, Notification>(
            "SELECT n.id, n.external_id, n.notification_type, n.title, n.body, n.date, n.course_id, COALESCE(c.custom_name, c.name, n.course_name) AS course_name, n.urgency, n.is_personal, n.is_read, n.url FROM notifications n LEFT JOIN courses c ON c.org_unit_id = n.course_id WHERE n.is_read = 0 AND n.course_id = ? ORDER BY n.date DESC NULLS LAST",
        ).bind(cid).fetch_all(pool).await?,
        (Some(cid), false) => sqlx::query_as::<_, Notification>(
            "SELECT n.id, n.external_id, n.notification_type, n.title, n.body, n.date, n.course_id, COALESCE(c.custom_name, c.name, n.course_name) AS course_name, n.urgency, n.is_personal, n.is_read, n.url FROM notifications n LEFT JOIN courses c ON c.org_unit_id = n.course_id WHERE n.course_id = ? ORDER BY n.date DESC NULLS LAST",
        ).bind(cid).fetch_all(pool).await?,
        (None, true) => sqlx::query_as::<_, Notification>(
            "SELECT n.id, n.external_id, n.notification_type, n.title, n.body, n.date, n.course_id, COALESCE(c.custom_name, c.name, n.course_name) AS course_name, n.urgency, n.is_personal, n.is_read, n.url FROM notifications n LEFT JOIN courses c ON c.org_unit_id = n.course_id WHERE n.is_read = 0 ORDER BY n.date DESC NULLS LAST",
        ).fetch_all(pool).await?,
        (None, false) => sqlx::query_as::<_, Notification>(
            "SELECT n.id, n.external_id, n.notification_type, n.title, n.body, n.date, n.course_id, COALESCE(c.custom_name, c.name, n.course_name) AS course_name, n.urgency, n.is_personal, n.is_read, n.url FROM notifications n LEFT JOIN courses c ON c.org_unit_id = n.course_id ORDER BY n.date DESC NULLS LAST LIMIT 200",
        ).fetch_all(pool).await?,
    };
    Ok(rows)
}

#[tauri::command]
pub async fn mark_notification_read(state: AppStateArg<'_>, id: i64) -> Result<()> {
    mark_notification_read_in(&state.pool, id).await?;
    state.events.notifications_updated();
    #[cfg(feature = "p2p")]
    if let Some(external_id) = read_external_id(&state, id).await {
        push_notification_read(&state, external_id, true).await;
    }
    Ok(())
}

/// Pool-only helper. Flips the row to is_read=1; no-op when the id is unknown.
pub async fn mark_notification_read_in(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("UPDATE notifications SET is_read = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn mark_all_notifications_read(
    state: AppStateArg<'_>,
    course_id: Option<String>,
) -> Result<()> {
    // Capture the set of unread external_ids BEFORE flipping them, so
    // we can mirror each one into Loro after the SQL UPDATE. Doing
    // the SELECT after would return zero rows. When a course_id is
    // supplied we scope to that course so the per-course Announcements
    // "Mark all read" doesn't silently clear unread state for every
    // other course on the dashboard.
    #[cfg(feature = "p2p")]
    let unread: Vec<String> = match course_id.as_deref() {
        Some(cid) => sqlx::query_scalar::<_, String>(
            "SELECT external_id FROM notifications WHERE is_read = 0 AND course_id = ?",
        )
        .bind(cid)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default(),
        None => sqlx::query_scalar::<_, String>(
            "SELECT external_id FROM notifications WHERE is_read = 0",
        )
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default(),
    };

    mark_all_notifications_read_in(&state.pool, course_id.as_deref()).await?;
    state.events.notifications_updated();

    #[cfg(feature = "p2p")]
    {
        for external_id in unread {
            push_notification_read(&state, external_id, true).await;
        }
    }
    Ok(())
}

/// Pool-only helper for `mark_all_notifications_read`. When `course_id` is
/// `Some`, only rows for that course are flipped to is_read=1; when `None`,
/// every unread row is flipped. Extracted so the course-scoping behavior can
/// be exercised in tests without spinning up an AppState/Tauri runtime.
pub async fn mark_all_notifications_read_in(
    pool: &SqlitePool,
    course_id: Option<&str>,
) -> Result<()> {
    match course_id {
        Some(cid) => {
            sqlx::query(
                "UPDATE notifications SET is_read = 1, updated_at = CURRENT_TIMESTAMP WHERE is_read = 0 AND course_id = ?",
            )
            .bind(cid)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query(
                "UPDATE notifications SET is_read = 1, updated_at = CURRENT_TIMESTAMP WHERE is_read = 0",
            )
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

// ---- p2p sync helpers (feature-gated) ------------------------------------

#[cfg(feature = "p2p")]
async fn read_external_id(state: &AppStateArg<'_>, id: i64) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT external_id FROM notifications WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()
}

#[cfg(feature = "p2p")]
async fn push_notification_read(state: &AppStateArg<'_>, external_id: String, read: bool) {
    if let Some(engine) = state.sync_engine() {
        if let Err(e) = engine
            .bridge()
            .apply_local(crate::p2p::bridge::LocalChange::NotificationRead {
                external_id: external_id.clone(),
                read,
            })
            .await
        {
            tracing::warn!("apply_local notification_read {}: {}", external_id, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite memory connect");
        // Minimal schema matching the production migration: just the columns
        // touched by the notifications read paths. We do not need the full
        // notifications table for these tests, only the columns we read or
        // mutate. Course join target is also minimal.
        sqlx::query(
            "CREATE TABLE courses (
                org_unit_id TEXT PRIMARY KEY,
                name TEXT,
                custom_name TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE notifications (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                external_id TEXT UNIQUE,
                notification_type TEXT,
                title TEXT,
                body TEXT,
                date TEXT,
                course_id TEXT,
                course_name TEXT,
                urgency INTEGER DEFAULT 0,
                is_personal INTEGER DEFAULT 0,
                is_read INTEGER DEFAULT 0,
                url TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn insert_alert(pool: &SqlitePool, external_id: &str, kind: &str, course_id: &str) {
        sqlx::query(
            "INSERT INTO notifications (external_id, notification_type, title, course_id, is_read)
             VALUES (?, ?, ?, ?, 0)",
        )
        .bind(external_id)
        .bind(kind)
        .bind(format!("{} alert", kind))
        .bind(course_id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn unread_count(pool: &SqlitePool, course_id: Option<&str>) -> i64 {
        match course_id {
            Some(cid) => sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM notifications WHERE is_read = 0 AND course_id = ?",
            )
            .bind(cid)
            .fetch_one(pool)
            .await
            .unwrap(),
            None => sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM notifications WHERE is_read = 0",
            )
            .fetch_one(pool)
            .await
            .unwrap(),
        }
    }

    // Mark-read by id resolves a single alert. This is the path the
    // Dashboard Notifications view and CourseAnnouncements view both
    // call into when the user clicks "Mark read" on a single row.
    #[tokio::test]
    async fn mark_notification_read_flips_single_row() {
        let pool = fresh_pool().await;
        insert_alert(&pool, "news_1_1", "News", "100").await;
        insert_alert(&pool, "alert_1_2", "Update", "100").await;
        assert_eq!(unread_count(&pool, None).await, 2);

        let target_id: i64 = sqlx::query_scalar("SELECT id FROM notifications WHERE external_id = ?")
            .bind("alert_1_2")
            .fetch_one(&pool)
            .await
            .unwrap();
        mark_notification_read_in(&pool, target_id).await.unwrap();

        assert_eq!(unread_count(&pool, None).await, 1);
        let other_read: i64 = sqlx::query_scalar(
            "SELECT is_read FROM notifications WHERE external_id = ?",
        )
        .bind("news_1_1")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(other_read, 0, "untouched row stayed unread");
    }

    // When the user clicks "Mark all read" from the Dashboard with no
    // course context, every unread Brightspace alert resolves regardless
    // of type (News update / Subscription / Message / etc).
    #[tokio::test]
    async fn mark_all_notifications_read_scoped_to_no_course_clears_everything() {
        let pool = fresh_pool().await;
        insert_alert(&pool, "news_100_1", "News", "100").await;        // News update
        insert_alert(&pool, "alert_100_2", "Subscription", "100").await; // Subscription
        insert_alert(&pool, "alert_100_3", "Message", "100").await;     // Message
        insert_alert(&pool, "alert_200_1", "Update", "200").await;      // other course
        assert_eq!(unread_count(&pool, None).await, 4);

        mark_all_notifications_read_in(&pool, None).await.unwrap();
        assert_eq!(unread_count(&pool, None).await, 0);
    }

    // When mark-all-read is invoked from a course context, only that
    // course's notifications are flipped. Before the course-scoped
    // patch, the SQL ignored course_id and silently cleared every
    // course's alerts.
    #[tokio::test]
    async fn mark_all_notifications_read_scoped_to_course_only_touches_that_course() {
        let pool = fresh_pool().await;
        insert_alert(&pool, "alert_100_1", "Update", "100").await;
        insert_alert(&pool, "alert_100_2", "Subscription", "100").await;
        insert_alert(&pool, "alert_200_1", "Update", "200").await;
        insert_alert(&pool, "alert_200_2", "Message", "200").await;
        assert_eq!(unread_count(&pool, None).await, 4);

        mark_all_notifications_read_in(&pool, Some("100"))
            .await
            .unwrap();

        assert_eq!(
            unread_count(&pool, Some("100")).await,
            0,
            "course 100 fully resolved",
        );
        assert_eq!(
            unread_count(&pool, Some("200")).await,
            2,
            "course 200 untouched",
        );
    }

    // list_notifications + course filter + unread_only returns Update,
    // Subscription, and Message alerts for a single course exactly once
    // and only the unread ones. Mirrors the per-course Announcements
    // surface, which is one of the two resolution endpoints called out
    // in the acceptance criteria.
    #[tokio::test]
    async fn list_notifications_course_filter_returns_unresolved_alerts_for_that_course() {
        let pool = fresh_pool().await;
        sqlx::query("INSERT INTO courses (org_unit_id, name) VALUES ('100', 'Course 100')")
            .execute(&pool)
            .await
            .unwrap();
        insert_alert(&pool, "alert_100_u", "Update", "100").await;
        insert_alert(&pool, "alert_100_s", "Subscription", "100").await;
        insert_alert(&pool, "alert_100_m", "Message", "100").await;
        insert_alert(&pool, "alert_200_u", "Update", "200").await;
        // Resolve one of the course-100 alerts so the unread filter has
        // something to exclude.
        let resolved: i64 =
            sqlx::query_scalar("SELECT id FROM notifications WHERE external_id = ?")
                .bind("alert_100_s")
                .fetch_one(&pool)
                .await
                .unwrap();
        mark_notification_read_in(&pool, resolved).await.unwrap();

        let rows = list_notifications_in(&pool, true, Some("100"))
            .await
            .unwrap();
        let mut kinds: Vec<String> = rows
            .iter()
            .map(|n| n.notification_type.clone())
            .collect();
        kinds.sort();
        assert_eq!(kinds, vec!["Message".to_string(), "Update".to_string()]);
        assert!(
            rows.iter().all(|n| n.course_id.as_deref() == Some("100")),
            "no cross-course leakage",
        );
    }
}
