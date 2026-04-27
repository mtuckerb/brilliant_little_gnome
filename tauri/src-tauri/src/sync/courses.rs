// Course/enrollment sync. Pulls /enrollments/myenrollments and upserts into
// `courses` while preserving user-edited fields (custom_color, units, target_grade,
// is_pinned, sort_order).

use crate::error::Result;
use crate::state::AppState;
use serde_json::Value;

pub async fn sync_enrollments(state: &AppState) -> Result<Vec<String>> {
    let enrollments = state.client.get_enrollments(&state.pool, false).await?;
    let mut ids = Vec::with_capacity(enrollments.len());

    for e in &enrollments {
        let Some(ou) = e.get("OrgUnit") else { continue; };
        let Some(id) = ou.get("Id").and_then(value_to_id) else { continue; };
        let name = ou.get("Name").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
        let code = ou.get("Code").and_then(|v| v.as_str()).map(|s| s.to_string());
        let semester = extract_semester(&name);
        let banner = e.pointer("/Access/ClasslistUrl").and_then(|v| v.as_str()).map(|s| s.to_string());
        let pin_date = e.get("PinDate").and_then(|v| v.as_str());
        let is_pinned = pin_date.is_some();
        let last_accessed = e.pointer("/Access/LastAccessed").and_then(|v| v.as_str()).map(|s| s.to_string());

        sqlx::query(
            "INSERT INTO courses (org_unit_id, name, code, semester, is_pinned, banner_url, last_accessed_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
             ON CONFLICT(org_unit_id) DO UPDATE SET
                name = excluded.name,
                code = COALESCE(excluded.code, courses.code),
                semester = COALESCE(excluded.semester, courses.semester),
                is_pinned = excluded.is_pinned,
                banner_url = COALESCE(excluded.banner_url, courses.banner_url),
                last_accessed_at = COALESCE(excluded.last_accessed_at, courses.last_accessed_at),
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind(&id)
        .bind(&name)
        .bind(code)
        .bind(semester)
        .bind(is_pinned as i64)
        .bind(banner)
        .bind(last_accessed)
        .execute(&state.pool)
        .await?;
        ids.push(id);
    }
    Ok(ids)
}

fn value_to_id(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() { return Some(s.to_string()); }
    if let Some(n) = v.as_i64() { return Some(n.to_string()); }
    if let Some(n) = v.as_f64() { return Some((n as i64).to_string()); }
    None
}

fn extract_semester(name: &str) -> Option<String> {
    let patterns = [
        regex::Regex::new(r"(?i)(\d{4}\s+(?:Spring|Fall|Summer|Winter|Session|Quarter))").unwrap(),
        regex::Regex::new(r"(?i)((?:Spring|Fall|Summer|Winter|Session|Quarter)\s+\d{4})").unwrap(),
    ];
    for p in &patterns {
        if let Some(m) = p.captures(name).and_then(|c| c.get(1)) {
            return Some(m.as_str().trim().to_string());
        }
    }
    None
}
