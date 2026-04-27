// Content/TOC sync. Fetches the course's table of contents and walks Modules→
// children, upserting `content_modules` and `content_items`. Critically scopes
// by `course_id` to avoid the cross-course module contamination bug we hit in
// the Ruby app — modules whose `brightspace_id` collides across courses are
// kept distinct because the unique key is `(course_id, brightspace_id)`.

use crate::error::Result;
use crate::state::AppState;
use serde_json::Value;

pub async fn sync(state: &AppState, course_id: &str) -> Result<()> {
    let toc = state.client.get_toc(&state.pool, course_id, false).await?;
    let modules = toc.get("Modules").and_then(|m| m.as_array()).cloned().unwrap_or_default();
    walk_modules(state, course_id, &modules, None, 0).await?;
    Ok(())
}

#[allow(clippy::manual_async_fn)]
fn walk_modules<'a>(
    state: &'a AppState,
    course_id: &'a str,
    modules: &'a [Value],
    parent_id: Option<&'a str>,
    depth: usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if depth > 6 { return Ok(()); }
        for (idx, m) in modules.iter().enumerate() {
            let Some(mid) = pick_id(m, &["Identifier", "ModuleId", "Id"]) else { continue; };
            let title = m.get("Title").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
            let description = m.pointer("/Description/Html").or_else(|| m.pointer("/Description/Text"))
                .and_then(|v| v.as_str()).map(|s| s.to_string());

            sqlx::query(
                "INSERT INTO content_modules (course_id, brightspace_id, title, description, sort_order, parent_id, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                 ON CONFLICT(course_id, brightspace_id) DO UPDATE SET
                    title = excluded.title,
                    description = COALESCE(excluded.description, content_modules.description),
                    sort_order = excluded.sort_order,
                    parent_id = excluded.parent_id,
                    updated_at = CURRENT_TIMESTAMP",
            )
            .bind(course_id)
            .bind(&mid)
            .bind(&title)
            .bind(description)
            .bind(idx as i64)
            .bind(parent_id)
            .execute(&state.pool)
            .await?;

            // Items (topics) under this module.
            if let Some(topics) = m.get("Topics").and_then(|t| t.as_array()) {
                for (i, t) in topics.iter().enumerate() {
                    let Some(tid) = pick_id(t, &["Identifier", "TopicId", "Id"]) else { continue; };
                    let ttitle = t.get("Title").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
                    let item_type = t.get("TypeIdentifier").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let url = t.get("Url").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let is_hidden = t.get("IsHidden").and_then(|v| v.as_bool()).unwrap_or(false);

                    sqlx::query(
                        "INSERT INTO content_items (module_id, brightspace_id, title, item_type, url, is_hidden, sort_order, created_at, updated_at)
                         VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                         ON CONFLICT(module_id, brightspace_id) DO UPDATE SET
                            title = excluded.title,
                            item_type = COALESCE(excluded.item_type, content_items.item_type),
                            url = COALESCE(excluded.url, content_items.url),
                            is_hidden = excluded.is_hidden,
                            sort_order = excluded.sort_order,
                            updated_at = CURRENT_TIMESTAMP",
                    )
                    .bind(&mid)
                    .bind(&tid)
                    .bind(&ttitle)
                    .bind(item_type)
                    .bind(url)
                    .bind(is_hidden as i64)
                    .bind(i as i64)
                    .execute(&state.pool)
                    .await?;
                }
            }

            // Recurse into nested Modules.
            if let Some(children) = m.get("Modules").and_then(|c| c.as_array()) {
                walk_modules(state, course_id, children, Some(&mid), depth + 1).await?;
            }
        }
        Ok(())
    })
}

fn pick_id(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(x) = v.get(*k) {
            if let Some(s) = x.as_str() { return Some(s.to_string()); }
            if let Some(n) = x.as_i64() { return Some(n.to_string()); }
        }
    }
    None
}
