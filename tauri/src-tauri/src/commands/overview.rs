// Course overview / syllabus commands. Mirrors the Sinatra app's
// `/course/:id/overview/{view,download}` routes: parse the cached JSON if we
// have it, otherwise fetch from Brightspace; surface either the attached file
// (PDF, etc.) or fall back to the HTML description.

use super::AppStateArg;
use crate::error::{AppError, Result};
use base64::Engine;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct CourseOverview {
    pub description_html: Option<String>,
    pub has_attachment: bool,
    pub attachment_name: Option<String>,
    pub attachment_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OverviewAttachment {
    pub bytes_base64: String,
    pub mime: Option<String>,
    pub filename: String,
}

/// Returns metadata about the course overview: HTML description (when present)
/// and attachment pointer (when present). Does not fetch the attachment bytes —
/// that's `fetch_course_overview_attachment`.
#[tauri::command]
pub async fn get_course_overview(state: AppStateArg<'_>, course_id: String) -> Result<CourseOverview> {
    let raw = load_or_fetch(&state, &course_id, false).await?;
    Ok(parse_overview(&raw))
}

/// Fetch the overview attachment (e.g. a syllabus PDF) and return it as
/// base64 so the frontend can save it via the file picker or open it via a
/// blob URL.
#[tauri::command]
pub async fn fetch_course_overview_attachment(
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<OverviewAttachment> {
    let raw = load_or_fetch(&state, &course_id, false).await?;
    let parsed = parse_overview(&raw);
    let url = parsed
        .attachment_url
        .ok_or_else(|| AppError::Other("overview has no attachment".to_string()))?;
    let suggested_name = parsed.attachment_name.unwrap_or_else(|| "Syllabus".to_string());

    let (bytes, mime, header_filename) = state.client.fetch_bytes(&url).await?;
    let filename = header_filename.unwrap_or(suggested_name);
    let bytes_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(OverviewAttachment { bytes_base64, mime, filename })
}

async fn load_or_fetch(state: &crate::state::AppState, course_id: &str, force: bool) -> Result<Value> {
    // Prefer the cached overview_raw on the course row when available; on miss
    // (or when the caller wants fresh) hit Brightspace and persist the result.
    if !force {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT overview_raw FROM courses WHERE org_unit_id = ?")
                .bind(course_id)
                .fetch_optional(&state.pool)
                .await?;
        if let Some((Some(json),)) = row {
            if let Ok(v) = serde_json::from_str::<Value>(&json) {
                return Ok(v);
            }
        }
    }
    let value = state.client.get_overview(&state.pool, course_id, force).await?;
    let json = serde_json::to_string(&value)?;
    sqlx::query("UPDATE courses SET overview_raw = ?, updated_at = CURRENT_TIMESTAMP WHERE org_unit_id = ?")
        .bind(json)
        .bind(course_id)
        .execute(&state.pool)
        .await?;
    Ok(value)
}

fn parse_overview(raw: &Value) -> CourseOverview {
    let description_html = raw
        .get("Description")
        .and_then(|d| {
            if let Some(s) = d.as_str() {
                Some(s.to_string())
            } else if let Some(o) = d.as_object() {
                o.get("Html")
                    .and_then(|v| v.as_str())
                    .or_else(|| o.get("Text").and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
            } else {
                None
            }
        });

    let has_attachment = raw.get("HasAttachment").and_then(|v| v.as_bool()).unwrap_or(false)
        || raw.pointer("/Attachment/Url").is_some();
    let attachment_url = raw.pointer("/Attachment/Url").and_then(|v| v.as_str()).map(|s| s.to_string());
    let attachment_name = raw.pointer("/Attachment/Name").and_then(|v| v.as_str()).map(|s| s.to_string());

    CourseOverview {
        description_html,
        has_attachment,
        attachment_name,
        attachment_url,
    }
}
