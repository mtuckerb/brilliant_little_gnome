// Notification sync. Pulls the unified feed and global alerts; upserts into
// `notifications` keyed by `external_id`.
//
// We track `user_preferences.last_notification_sync_at` and pass it as `?since=`
// to both endpoints so each subsequent sync only fetches deltas. The very first
// sync (or any sync after a manual reset) sends no `since` and pulls the full
// window the server returns.

use crate::error::Result;
use crate::state::AppState;
use serde_json::Value;

pub async fn sync(state: &AppState, full_refresh: bool) -> Result<()> {
    // Snapshot the previous high-water mark BEFORE we start fetching, so the
    // window we use to filter is stable across the two endpoint calls.
    //
    // `full_refresh` drops the `?since=` delta entirely so the server returns
    // its full recent window. Used when a course just became available: its
    // announcements were posted before our global high-water mark, so a delta
    // pull would never see them. Upserts are idempotent, so re-pulling the
    // window only refreshes existing rows.
    let since: Option<String> = if full_refresh {
        None
    } else {
        sqlx::query_scalar("SELECT last_notification_sync_at FROM user_preferences LIMIT 1")
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
    };
    let since_ref = since.as_deref();

    // Unified feed.
    let feed = state
        .client
        .get_unified_feed(&state.pool, since_ref, true)
        .await
        .unwrap_or_default();
    for item in &feed {
        upsert_feed_item(state, item).await.ok();
    }

    // Global alerts.
    let alerts = state
        .client
        .get_global_alerts(&state.pool, since_ref)
        .await
        .unwrap_or_default();
    for a in &alerts {
        upsert_alert(state, a).await.ok();
    }

    // Advance the high-water mark only on a clean fetch path. We use the local
    // clock rather than a server-provided timestamp because Brightspace doesn't
    // return one consistently; clock skew is acceptable for a delta window.
    let _ = sqlx::query(
        "UPDATE user_preferences SET last_notification_sync_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT id FROM user_preferences LIMIT 1)",
    )
    .execute(&state.pool)
    .await;

    state.events.notifications_updated();
    Ok(())
}

async fn upsert_feed_item(state: &AppState, item: &Value) -> Result<()> {
    let metadata = item.get("Metadata");
    let resource = item.get("Resource");
    let title = metadata.and_then(|m| m.get("Title")).and_then(|v| v.as_str()).unwrap_or("News update").to_string();
    let body = metadata.and_then(|m| m.pointer("/Summary/Text")).and_then(|v| v.as_str()).map(|s| s.to_string());
    let date = metadata.and_then(|m| m.get("Date")).and_then(|v| v.as_str())
        .or_else(|| resource.and_then(|r| r.get("LastModifiedDate")).and_then(|v| v.as_str()))
        .or_else(|| resource.and_then(|r| r.get("CreatedDate")).and_then(|v| v.as_str()))
        .map(|s| s.to_string());

    let api_view = metadata.and_then(|m| m.get("ApiViewUrl")).and_then(|v| v.as_str()).unwrap_or("");
    let course_id = regex::Regex::new(r"/(\d+)/news").unwrap()
        .captures(api_view).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
    let news_id = metadata.and_then(|m| m.get("Identifier")).and_then(|v| v.as_str()).map(|s| s.to_string())
        .or_else(|| metadata.and_then(|m| m.get("Identifier")).and_then(|v| v.as_i64()).map(|n| n.to_string()));
    let external_id = match (&course_id, &news_id) {
        (Some(c), Some(n)) => format!("news_{}_{}", c, n),
        (None, Some(n)) => format!("news_{}", n),
        _ => return Ok(()),
    };

    let url = metadata.and_then(|m| m.get("WebViewUrl")).and_then(|v| v.as_str()).map(|s| s.to_string());

    sqlx::query(
        "INSERT INTO notifications (external_id, notification_type, title, body, date, course_id, urgency, is_personal, is_read, url, created_at, updated_at)
         VALUES (?, 'News', ?, ?, ?, ?, 1, 0, 0, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(external_id) DO UPDATE SET
            title = excluded.title,
            body = excluded.body,
            date = excluded.date,
            course_id = excluded.course_id,
            url = excluded.url,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&external_id)
    .bind(&title)
    .bind(body)
    .bind(date)
    .bind(course_id)
    .bind(url)
    .execute(&state.pool)
    .await?;

    // T-011: drain any pending notification overlay (is_read flag) for this row.
    #[cfg(feature = "p2p")]
    if let Err(e) = crate::p2p::bridge::drain_pending_overlay(
        &state.pool,
        crate::p2p::bridge::OverlayKind::Notification,
        &external_id,
    )
    .await
    {
        tracing::warn!("drain pending notification overlay {}: {}", external_id, e);
    }

    Ok(())
}

async fn upsert_alert(state: &AppState, alert: &Value) -> Result<()> {
    let alert_type = alert.get("Type").and_then(|v| v.as_str()).unwrap_or("Alert");
    let course_id = alert.get("OrgUnitId").and_then(|v| v.as_i64()).map(|n| n.to_string())
        .or_else(|| alert.get("OrgUnitId").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let id = alert.get("Id").and_then(|v| v.as_i64()).map(|n| n.to_string())
        .or_else(|| alert.get("Id").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let (Some(cid), Some(aid)) = (course_id, id) else { return Ok(()); };
    let external_id = format!("alert_{}_{}", cid, aid);

    let title = alert.get("Title").and_then(|v| v.as_str()).map(|s| s.to_string())
        .unwrap_or_else(|| format!("{} Update", alert_type));
    let body = alert.get("Text").and_then(|v| v.as_str()).map(|s| s.to_string());
    let date = alert.get("Date").and_then(|v| v.as_str()).map(|s| s.to_string());
    let urgency = if alert_type == "Grade" { 3 } else { 1 };
    let is_personal = (alert_type == "Grade") as i64;
    let url = if alert_type == "Grade" {
        format!("/course/{}/grades", cid)
    } else {
        format!("/course/{}", cid)
    };

    sqlx::query(
        "INSERT INTO notifications (external_id, notification_type, title, body, date, course_id, urgency, is_personal, is_read, url, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(external_id) DO UPDATE SET
            title = excluded.title,
            body = excluded.body,
            date = excluded.date,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&external_id)
    .bind(alert_type)
    .bind(&title)
    .bind(body)
    .bind(date)
    .bind(&cid)
    .bind(urgency)
    .bind(is_personal)
    .bind(url)
    .execute(&state.pool)
    .await?;

    // T-011: drain any pending notification overlay (is_read flag) for this row.
    #[cfg(feature = "p2p")]
    if let Err(e) = crate::p2p::bridge::drain_pending_overlay(
        &state.pool,
        crate::p2p::bridge::OverlayKind::Notification,
        &external_id,
    )
    .await
    {
        tracing::warn!("drain pending notification overlay {}: {}", external_id, e);
    }

    Ok(())
}
