use super::AppStateArg;
use crate::commands::assignment_detail::{PreviewAttachment, PREVIEW_MAX_BYTES};
use crate::error::Result;
use crate::models::{ContentItem, ContentModule};
use base64::Engine;

#[tauri::command]
pub async fn list_modules(state: AppStateArg<'_>, course_id: String) -> Result<Vec<ContentModule>> {
    let rows = sqlx::query_as::<_, ContentModule>(
        "SELECT id, course_id, brightspace_id, title, description, sort_order, parent_id
         FROM content_modules WHERE course_id = ? ORDER BY parent_id IS NOT NULL, sort_order, title",
    )
    .bind(&course_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

/// Every item in a course, in one round-trip. The tree view on the Modules
/// page renders all modules and their contents at once, so fetching per
/// module would mean one `invoke` per node (40+ on a big course). Items are
/// keyed to their module by `module_id` -> `content_modules.brightspace_id`;
/// the subquery (rather than a JOIN) keeps the result one row per item even
/// if a module id were ever to repeat.
#[tauri::command]
pub async fn list_course_items(state: AppStateArg<'_>, course_id: String) -> Result<Vec<ContentItem>> {
    let rows = sqlx::query_as::<_, ContentItem>(
        "SELECT id, module_id, brightspace_id, title, item_type, url, is_hidden, sort_order
         FROM content_items
         WHERE module_id IN (SELECT brightspace_id FROM content_modules WHERE course_id = ?)
         ORDER BY sort_order, title",
    )
    .bind(&course_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}

/// Fetch a content topic's file for in-app preview. Same Brightspace endpoint
/// as `download_topic_file`, but returns the bytes instead of writing them to
/// the downloads directory, and caps the size like the attachment previewer.
/// Going through the topic id (rather than the item's stored `url`) means the
/// 63 topics with no recorded URL still open, and the server's
/// Content-Disposition gives us the real filename — and so the real extension
/// — which is what the viewer routes on.
#[tauri::command]
pub async fn preview_topic_file(
    state: AppStateArg<'_>,
    course_id: String,
    topic_id: String,
) -> Result<PreviewAttachment> {
    let path = format!(
        "/d2l/api/le/{}/{}/content/topics/{}/file",
        crate::client::API_VERSION,
        course_id,
        topic_id
    );
    let (bytes, mime, header_filename) = state
        .client
        .fetch_bytes_with_limit(&path, Some(PREVIEW_MAX_BYTES))
        .await?;
    Ok(PreviewAttachment {
        bytes_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        mime,
        filename: header_filename.unwrap_or_else(|| format!("topic_{}", topic_id)),
    })
}

#[tauri::command]
pub async fn list_items(state: AppStateArg<'_>, module_id: String) -> Result<Vec<ContentItem>> {
    let rows = sqlx::query_as::<_, ContentItem>(
        "SELECT id, module_id, brightspace_id, title, item_type, url, is_hidden, sort_order
         FROM content_items WHERE module_id = ? ORDER BY sort_order, title",
    )
    .bind(&module_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows)
}
