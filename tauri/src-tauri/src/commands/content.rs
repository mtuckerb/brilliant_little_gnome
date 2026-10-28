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
    super::zotero::schedule_document_backfill(&state).await;
    let mut rows = sqlx::query_as::<_, ContentItem>(
        "SELECT id, module_id, brightspace_id, title, item_type, url, is_hidden, sort_order
         FROM content_items
         WHERE module_id IN (SELECT brightspace_id FROM content_modules WHERE course_id = ?)
         ORDER BY sort_order, title",
    )
    .bind(&course_id)
    .fetch_all(&state.pool)
    .await?;
    overlay_zotero_links(&state.pool, &course_id, &mut rows).await?;
    Ok(rows)
}

async fn overlay_zotero_links(
    pool: &sqlx::SqlitePool,
    course_id: &str,
    items: &mut [ContentItem],
) -> Result<()> {
    let links = super::zotero::document_links(pool, course_id).await?;
    for item in items {
        item.zotero_url = links.get(&item.brightspace_id).cloned();
    }
    Ok(())
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
    app: tauri::AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
    topic_id: String,
) -> Result<PreviewAttachment> {
    // Cache-first: if this topic was made available offline, serve the stored
    // bytes (instant, works with no network). Falls through to a live fetch
    // when caching is off or the topic isn't cached.
    if let Some(cached) =
        crate::commands::content_cache::get_cached(&app, &state.pool, &course_id, &topic_id).await?
    {
        return Ok(PreviewAttachment {
            bytes_base64: base64::engine::general_purpose::STANDARD.encode(&cached.bytes),
            mime: cached.mime,
            filename: cached.filename,
        });
    }
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
pub async fn list_items(
    state: AppStateArg<'_>,
    module_id: String,
    course_id: Option<String>,
) -> Result<Vec<ContentItem>> {
    super::zotero::schedule_document_backfill(&state).await;
    let mut rows = sqlx::query_as::<_, ContentItem>(
        "SELECT id, module_id, brightspace_id, title, item_type, url, is_hidden, sort_order
         FROM content_items WHERE module_id = ? ORDER BY sort_order, title",
    )
    .bind(&module_id)
    .fetch_all(&state.pool)
    .await?;
    // Older callers omit course_id; only infer it when the module is unique.
    let course_id = match course_id {
        Some(id) => Some(id),
        None => sqlx::query_scalar::<_, String>(
            "SELECT course_id FROM content_modules WHERE brightspace_id = ?
             GROUP BY brightspace_id HAVING COUNT(*) = 1",
        ).bind(&module_id).fetch_optional(&state.pool).await?,
    };
    if let Some(id) = course_id {
        overlay_zotero_links(&state.pool, &id, &mut rows).await?;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zotero::{ZoteroClient, ZoteroMode};

    #[tokio::test]
    async fn zotero_overlay_keeps_the_source_and_survives_brightspace_resync() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_preferences (api_port) VALUES (4567)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO content_items (module_id, brightspace_id, title, url) VALUES ('module', '123', 'Reading', 'https://school.test/old.pdf')")
            .execute(&pool).await.unwrap();
        let mut items = sqlx::query_as::<_, ContentItem>("SELECT * FROM content_items")
            .fetch_all(&pool).await.unwrap();
        overlay_zotero_links(&pool, "course", &mut items).await.unwrap();
        assert!(items[0].zotero_url.is_none(), "unsent or failed documents have no overlay");

        let zc = ZoteroClient::new(ZoteroMode::Local {
            base_url: None, basic_auth: None, user_id_override: None, api_key: None,
        }).await.unwrap();
        zc.remember_document(&pool, "course", "123", "ABCD1234", Some("application/pdf"))
            .await.unwrap();
        sqlx::query("UPDATE content_items SET url = 'https://school.test/new.pdf'")
            .execute(&pool).await.unwrap();
        let mut items = sqlx::query_as::<_, ContentItem>("SELECT * FROM content_items")
            .fetch_all(&pool).await.unwrap();
        overlay_zotero_links(&pool, "course", &mut items).await.unwrap();
        assert_eq!(items[0].url.as_deref(), Some("https://school.test/new.pdf"));
        assert_eq!(items[0].zotero_url.as_deref(), Some("zotero://open-pdf/library/items/ABCD1234"));

        overlay_zotero_links(&pool, "other-course", &mut items).await.unwrap();
        assert!(items[0].zotero_url.is_none());
        sqlx::query("UPDATE user_preferences SET zotero_use_local = 0")
            .execute(&pool).await.unwrap();
        overlay_zotero_links(&pool, "course", &mut items).await.unwrap();
        assert!(items[0].zotero_url.is_none(), "incomplete cloud setup uses Brightspace");
        sqlx::query("UPDATE user_preferences SET zotero_user_id = '42', zotero_api_key = 'secret'")
            .execute(&pool).await.unwrap();
        overlay_zotero_links(&pool, "course", &mut items).await.unwrap();
        assert!(items[0].zotero_url.is_none(), "another library cannot reuse local links");
    }
}
