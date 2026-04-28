// Discussion sync. Mirrors `Brilliant::Sync::DiscussionService`:
//   1. Upsert forums for the course.
//   2. Try the all-topics endpoint `/<course>/discussions/topics/`. If it
//      returns data, use that (forum_id comes off each topic). Otherwise fall
//      back to per-forum topic fetches.
// Posts/threads are intentionally not synced — the Ruby app doesn't either.

use crate::client::ensure_array;
use crate::error::Result;
use crate::state::AppState;
use serde_json::Value;

pub async fn sync(state: &AppState, course_id: &str) -> Result<()> {
    let forums = state.client.get_discussion_forums(&state.pool, course_id, false).await.unwrap_or_default();
    for f in &forums {
        upsert_forum(state, course_id, f).await.ok();
    }

    // Try the bulk topics endpoint first.
    let bulk_path = format!("/d2l/api/le/1.40/{}/discussions/topics/", course_id);
    let bulk = state.client.do_get(&state.pool, &bulk_path, false).await.ok();
    let used_bulk = match bulk {
        Some(v) => {
            let topics = ensure_array(&v);
            if !topics.is_empty() {
                for (i, t) in topics.iter().enumerate() {
                    let fid = t.get("ForumId").and_then(|v| v.as_i64()).map(|n| n.to_string())
                        .or_else(|| t.get("ForumId").and_then(|v| v.as_str()).map(|s| s.to_string()));
                    if let Some(fid) = fid {
                        upsert_topic(state, course_id, &fid, i, t).await.ok();
                    }
                }
                true
            } else {
                false
            }
        }
        None => false,
    };

    if !used_bulk {
        // Fall back to per-forum fetches.
        for f in &forums {
            let Some(forum_id) = f.get("ForumId").and_then(|v| v.as_i64()).map(|n| n.to_string())
                .or_else(|| f.get("ForumId").and_then(|v| v.as_str()).map(|s| s.to_string())) else { continue };
            let topics = state.client.get_discussion_topics(&state.pool, course_id, &forum_id, false).await.unwrap_or_default();
            for (i, t) in topics.iter().enumerate() {
                upsert_topic(state, course_id, &forum_id, i, t).await.ok();
            }
        }
    }

    Ok(())
}

async fn upsert_forum(state: &AppState, course_id: &str, f: &Value) -> Result<()> {
    let Some(forum_id) = f.get("ForumId").and_then(|v| v.as_i64()).map(|n| n.to_string())
        .or_else(|| f.get("ForumId").and_then(|v| v.as_str()).map(|s| s.to_string())) else { return Ok(()); };
    let name = f.get("Name").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
    let description = description_text(f.get("Description"));

    sqlx::query(
        "INSERT INTO discussion_forums (brightspace_id, course_id, name, description, created_at, updated_at)
         VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(course_id, brightspace_id) DO UPDATE SET
            name = excluded.name,
            description = COALESCE(excluded.description, discussion_forums.description),
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&forum_id)
    .bind(course_id)
    .bind(&name)
    .bind(description)
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn upsert_topic(state: &AppState, course_id: &str, forum_id: &str, sort_order: usize, t: &Value) -> Result<()> {
    let Some(topic_id) = t.get("TopicId").and_then(|v| v.as_i64()).map(|n| n.to_string())
        .or_else(|| t.get("TopicId").and_then(|v| v.as_str()).map(|s| s.to_string())) else { return Ok(()); };
    let name = t.get("Name").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
    let description = description_text(t.get("Description"));
    let thread_count = t.get("ThreadCount").and_then(|v| v.as_i64());
    let post_count = t.get("PostCount").and_then(|v| v.as_i64());
    let last_post_date = t.get("LastPostDate").and_then(|v| v.as_str()).map(|s| s.to_string());

    sqlx::query(
        "INSERT INTO discussion_topics (brightspace_id, course_id, forum_id, name, description, sort_order, thread_count, post_count, last_post_date, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, COALESCE(?, 0), COALESCE(?, 0), ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(forum_id, brightspace_id) DO UPDATE SET
            course_id = excluded.course_id,
            name = excluded.name,
            description = COALESCE(excluded.description, discussion_topics.description),
            sort_order = excluded.sort_order,
            thread_count = COALESCE(excluded.thread_count, discussion_topics.thread_count),
            post_count = COALESCE(excluded.post_count, discussion_topics.post_count),
            last_post_date = COALESCE(excluded.last_post_date, discussion_topics.last_post_date),
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&topic_id)
    .bind(course_id)
    .bind(forum_id)
    .bind(&name)
    .bind(description)
    .bind(sort_order as i64)
    .bind(thread_count)
    .bind(post_count)
    .bind(last_post_date)
    .execute(&state.pool)
    .await?;
    Ok(())
}

fn description_text(d: Option<&Value>) -> Option<String> {
    let d = d?;
    if let Some(s) = d.as_str() { return Some(s.to_string()); }
    d.get("Html").and_then(|v| v.as_str()).map(|s| s.to_string())
        .or_else(|| d.get("Text").and_then(|v| v.as_str()).map(|s| s.to_string()))
}
