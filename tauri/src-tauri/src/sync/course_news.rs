// Per-course announcement ("News") sync.
//
// Brightspace's global `/lp/feed/` only surfaces a *subset* of course
// announcements, so relying on it alone silently drops things like a "room
// change for tomorrow" notice. This pulls each course's announcements directly
// from `/le/{ver}/{course}/news/` — the authoritative, complete list — and
// upserts them with the same `news_{course}_{id}` external_id the feed path
// uses, so the two sources dedupe instead of double-listing.

use crate::error::Result;
use crate::state::AppState;
use serde_json::Value;

pub async fn sync(state: &AppState, course_id: &str) -> Result<()> {
    let items = state.client.get_course_news(&state.pool, course_id, false).await?;
    let mut upserted = 0u32;
    for item in &items {
        match upsert_news_item(state, course_id, item).await {
            Ok(true) => upserted += 1,
            Ok(false) => {}
            Err(e) => tracing::warn!("[{}] news upsert failed: {}", course_id, e),
        }
    }
    if upserted > 0 {
        // sync_course (single-course path) has no global notifications step, so
        // emit here too; the per-course concurrent fan-out may emit a few times
        // during a full sync, which the UI coalesces on reload.
        state.events.notifications_updated();
    }
    Ok(())
}

async fn upsert_news_item(state: &AppState, course_id: &str, item: &Value) -> Result<bool> {
    if !news_is_visible(item) {
        return Ok(false);
    }

    let Some(id) = item.get("Id").and_then(value_to_id) else {
        return Ok(false);
    };
    let external_id = format!("news_{}_{}", course_id, id);
    let title = item
        .get("Title")
        .and_then(|v| v.as_str())
        .unwrap_or("Announcement")
        .to_string();
    // News bodies come back as a structured object: prefer the plain text,
    // fall back to the HTML.
    let body = item.get("Body").and_then(|b| {
        b.get("Text")
            .and_then(|v| v.as_str())
            .or_else(|| b.get("Html").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    });
    let date = item
        .get("StartDate")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // ON CONFLICT preserves is_read and url so re-syncing never re-marks an
    // announcement unread or clobbers a feed-sourced web link.
    sqlx::query(
        "INSERT INTO notifications (external_id, notification_type, title, body, date, course_id, urgency, is_personal, is_read, url, created_at, updated_at)
         VALUES (?, 'News', ?, ?, ?, ?, 1, 0, 0, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(external_id) DO UPDATE SET
            title = excluded.title,
            body = excluded.body,
            date = excluded.date,
            course_id = excluded.course_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&external_id)
    .bind(&title)
    .bind(body)
    .bind(date)
    .bind(course_id)
    .execute(&state.pool)
    .await?;

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

    Ok(true)
}

/// Mirror Brightspace's student-facing visibility: skip drafts, instructor-
/// hidden items, announcements scheduled for the future, and expired ones.
fn news_is_visible(item: &Value) -> bool {
    if item.get("IsPublished").and_then(|v| v.as_bool()) == Some(false) {
        return false;
    }
    if item.get("IsHidden").and_then(|v| v.as_bool()) == Some(true) {
        return false;
    }
    if let Some(start) = item.get("StartDate").and_then(|v| v.as_str()) {
        if is_future(start) {
            return false;
        }
    }
    if let Some(end) = item.get("EndDate").and_then(|v| v.as_str()) {
        if is_past(end) {
            return false;
        }
    }
    true
}

fn value_to_id(v: &Value) -> Option<String> {
    v.as_i64()
        .map(|n| n.to_string())
        .or_else(|| v.as_str().map(|s| s.to_string()))
}

fn is_future(ts: &str) -> bool {
    parse(ts).map_or(false, |t| t > chrono::Utc::now())
}

fn is_past(ts: &str) -> bool {
    parse(ts).map_or(false, |t| t < chrono::Utc::now())
}

fn parse(ts: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

#[cfg(test)]
mod tests {
    use super::{news_is_visible, value_to_id};
    use serde_json::json;

    #[test]
    fn room_change_announcement_is_visible() {
        // Real shape from /le/1.40/{course}/news/ for "Classroom for tomorrow".
        let item = json!({
            "Id": 1251052,
            "Title": "Classroom for tomorrow",
            "Body": { "Html": "<p>Room 214</p>", "Text": "Room 214" },
            "StartDate": "2026-06-30T17:18:00.000Z",
            "EndDate": null,
            "IsPublished": true,
            "IsHidden": false
        });
        assert!(news_is_visible(&item));
        assert_eq!(value_to_id(item.get("Id").unwrap()).as_deref(), Some("1251052"));
    }

    #[test]
    fn drafts_and_hidden_are_skipped() {
        assert!(!news_is_visible(&json!({ "Id": 1, "IsPublished": false })));
        assert!(!news_is_visible(&json!({ "Id": 2, "IsHidden": true })));
    }

    #[test]
    fn scheduled_future_and_expired_are_skipped() {
        assert!(!news_is_visible(&json!({ "Id": 3, "StartDate": "2999-01-01T00:00:00.000Z" })));
        assert!(!news_is_visible(&json!({ "Id": 4, "EndDate": "2000-01-01T00:00:00.000Z" })));
    }

    #[test]
    fn missing_optional_fields_defaults_visible() {
        // No IsPublished/IsHidden/dates -> treat as a live announcement.
        assert!(news_is_visible(&json!({ "Id": 5, "Title": "Welcome!" })));
    }
}
