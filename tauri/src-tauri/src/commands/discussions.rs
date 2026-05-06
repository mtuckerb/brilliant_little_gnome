use super::AppStateArg;
use crate::error::{AppError, Result};
use crate::models::{DiscussionForum, DiscussionTopic};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct DiscussionPostView {
    pub post_id: String,
    pub parent_post_id: Option<String>,
    pub thread_id: Option<String>,
    pub subject: Option<String>,
    pub body_html: Option<String>,
    pub author_name: Option<String>,
    pub author_id: Option<String>,
    pub posted_at: Option<String>,
    pub is_pinned: bool,
}

#[tauri::command]
pub async fn list_forums(state: AppStateArg<'_>, course_id: String) -> Result<Vec<DiscussionForum>> {
    let rows = sqlx::query_as::<_, DiscussionForum>(
        "SELECT id, brightspace_id, course_id, name, description
         FROM discussion_forums WHERE course_id = ? ORDER BY name",
    )
    .bind(&course_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

#[tauri::command]
pub async fn list_topics(state: AppStateArg<'_>, course_id: String) -> Result<Vec<DiscussionTopic>> {
    let rows = sqlx::query_as::<_, DiscussionTopic>(
        "SELECT id, brightspace_id, course_id, forum_id, name, description, sort_order, thread_count, post_count, last_post_date
         FROM discussion_topics WHERE course_id = ? ORDER BY last_post_date DESC NULLS LAST, sort_order, name",
    )
    .bind(&course_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

/// Fetch posts for a topic on-demand. Posts/threads aren't synced into the DB
/// (they'd balloon the cache for limited value) — we hit Brightspace each time
/// and let the HTTP cache layer handle freshness.
#[tauri::command]
pub async fn list_topic_posts(
    state: AppStateArg<'_>,
    course_id: String,
    topic_id: String,
) -> Result<Vec<DiscussionPostView>> {
    // Find the forum_id locally so the caller doesn't have to plumb it.
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT forum_id FROM discussion_topics WHERE course_id = ? AND brightspace_id = ?",
    )
    .bind(&course_id)
    .bind(&topic_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((forum_id,)) = row else {
        return Err(AppError::Other(format!(
            "topic {} not found in course {}",
            topic_id, course_id
        )));
    };

    let raw = state
        .client
        .get_discussion_posts(&state.pool, &course_id, &forum_id, &topic_id, false)
        .await?;
    Ok(raw.iter().map(parse_post).collect())
}

fn parse_post(p: &Value) -> DiscussionPostView {
    let post_id = p
        .get("PostId")
        .and_then(value_to_id)
        .unwrap_or_else(|| "0".to_string());
    let parent_post_id = p.get("ParentPostId").and_then(value_to_id);
    let thread_id = p.get("ThreadId").and_then(value_to_id);
    let subject = p.get("Subject").and_then(|v| v.as_str()).map(|s| s.to_string());
    let body_html = p.get("Message").and_then(|v| {
        if let Some(s) = v.as_str() { return Some(s.to_string()); }
        v.get("Html").and_then(|h| h.as_str()).map(|s| s.to_string())
            .or_else(|| v.get("Text").and_then(|h| h.as_str()).map(|s| s.to_string()))
    });
    let author_name = p
        .pointer("/PostingUser/DisplayName")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let author_id = p
        .pointer("/PostingUser/Identifier")
        .and_then(value_to_id)
        .or_else(|| p.get("PostingUserId").and_then(value_to_id));
    let posted_at = p
        .get("DatePosted")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let is_pinned = p.get("IsPinned").and_then(|v| v.as_bool()).unwrap_or(false);

    DiscussionPostView {
        post_id,
        parent_post_id,
        thread_id,
        subject,
        body_html,
        author_name,
        author_id,
        posted_at,
        is_pinned,
    }
}

fn value_to_id(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() { return Some(s.to_string()); }
    if let Some(n) = v.as_i64() { return Some(n.to_string()); }
    if let Some(n) = v.as_f64() { return Some((n as i64).to_string()); }
    None
}
