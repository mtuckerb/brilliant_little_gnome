use super::AppStateArg;
use crate::error::Result;
use crate::models::{DiscussionForum, DiscussionTopic};

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
