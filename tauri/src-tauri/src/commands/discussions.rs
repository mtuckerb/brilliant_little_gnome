use super::AppStateArg;
use crate::error::{AppError, Result};
use crate::models::{DiscussionForum, DiscussionTopic};
use serde::Serialize;
use serde_json::{json, Value};

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

pub(crate) fn parse_post(p: &Value) -> DiscussionPostView {
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
    // Brightspace puts the author at the top level as `PostingUserDisplayName`
    // (and `PostingUserId`), NOT nested inside PostingUser. The nested form
    // is only returned for /whoami-style endpoints. The previous parse
    // landed on null for every post, which the React side then rendered as
    // "unknown" and which mis-attributed replies in the threaded view
    // (every reply collapsed to the same fallback).
    let author_name = p
        .get("PostingUserDisplayName")
        .and_then(|v| v.as_str())
        .or_else(|| p.pointer("/PostingUser/DisplayName").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .or_else(|| {
            // Last-ditch: build "First Last" from any FirstName + LastName
            // pair Brightspace happens to expose.
            let first = p
                .pointer("/PostingUser/FirstName")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let last = p
                .pointer("/PostingUser/LastName")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let combined = format!("{} {}", first, last).trim().to_string();
            if combined.is_empty() { None } else { Some(combined) }
        });
    let author_id = p
        .get("PostingUserId")
        .and_then(value_to_id)
        .or_else(|| p.pointer("/PostingUser/Identifier").and_then(value_to_id));
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

#[derive(Debug, Serialize, Default)]
pub struct MarkReadResult {
    pub marked: usize,
    pub failed: usize,
}

/// Mark every post in a topic as read, propagating to Brightspace via the
/// per-post `MyReadStatus` PUT. Returns counts so the UI can surface
/// partial failures without aborting the whole batch.
#[tauri::command]
pub async fn mark_topic_read(
    state: AppStateArg<'_>,
    course_id: String,
    topic_id: String,
) -> Result<MarkReadResult> {
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
    mark_posts_read_inner(&state, &course_id, &topic_id, &raw).await
}

/// Mark every post in every topic of a course as read. Walks topics and
/// hits each one's posts; tolerates per-topic failures so a single broken
/// forum doesn't halt the rest.
#[tauri::command]
pub async fn mark_course_discussions_read(
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<MarkReadResult> {
    let topics: Vec<(String, String)> = sqlx::query_as(
        "SELECT brightspace_id, forum_id FROM discussion_topics WHERE course_id = ?",
    )
    .bind(&course_id)
    .fetch_all(&state.pool)
    .await?;
    let mut total = MarkReadResult::default();
    for (topic_id, forum_id) in topics {
        let raw = match state
            .client
            .get_discussion_posts(&state.pool, &course_id, &forum_id, &topic_id, false)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("topic {} fetch failed: {}", topic_id, e);
                total.failed += 1;
                continue;
            }
        };
        let r = mark_posts_read_inner(&state, &course_id, &topic_id, &raw).await?;
        total.marked += r.marked;
        total.failed += r.failed;
    }
    Ok(total)
}

async fn mark_posts_read_inner(
    state: &AppStateArg<'_>,
    course_id: &str,
    topic_id: &str,
    posts: &[Value],
) -> Result<MarkReadResult> {
    let mut result = MarkReadResult::default();
    for p in posts {
        // Only mark unread posts — Brightspace doesn't error on already-
        // read, but skipping saves API calls.
        let is_read = p.get("IsRead").and_then(|v| v.as_bool()).unwrap_or(false);
        if is_read {
            continue;
        }
        let Some(post_id) = p.get("PostId").and_then(value_to_id) else { continue };
        let path = format!(
            "/d2l/api/le/{}/{}/discussions/topics/{}/posts/{}/MyReadStatus",
            crate::client::API_VERSION,
            course_id,
            topic_id,
            post_id,
        );
        match state.client.put_json(&path, json!({ "IsRead": true })).await {
            Ok(()) => result.marked += 1,
            Err(e) => {
                tracing::warn!("mark post {} read failed: {}", post_id, e);
                result.failed += 1;
            }
        }
    }
    Ok(result)
}
