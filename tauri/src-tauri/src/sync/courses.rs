// Course/enrollment sync. Pulls /enrollments/myenrollments and upserts into
// `courses` while preserving user-edited fields (custom_color, units, target_grade,
// is_pinned, sort_order).

use crate::error::Result;
use crate::state::AppState;
use serde_json::Value;

pub async fn sync_enrollments(state: &AppState) -> Result<Vec<String>> {
    let enrollments = state.client.get_enrollments(&state.pool, false).await?;
    let host = state.client.host_clone();
    let mut ids = Vec::with_capacity(enrollments.len());

    for e in &enrollments {
        let Some(ou) = e.get("OrgUnit") else { continue; };
        let Some(id) = ou.get("Id").and_then(value_to_id) else { continue; };
        let name = ou.get("Name").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
        // Brightspace's `OrgUnit.Code` at this institution is a section/banner code
        // like "2620.UMS06-S.40963.1" — useless to students. We instead extract a
        // friendly code ("SWO-350", "PSY 220") from the course Name and fall back
        // to the raw Code only when extraction fails.
        let friendly_code = extract_course_code(&name);
        let raw_code = ou.get("Code").and_then(|v| v.as_str()).map(|s| s.to_string());
        let code = friendly_code.or(raw_code);
        let semester = extract_semester(&name);
        let banner = extract_banner(ou, host.as_deref());
        let pin_date = e.get("PinDate").and_then(|v| v.as_str());
        let is_pinned = pin_date.is_some();
        let last_accessed = e.pointer("/Access/LastAccessed").and_then(|v| v.as_str()).map(|s| s.to_string());

        // NB: `code` and `banner_url` are *not* COALESCE-preserved any more — sync
        // is now the source of truth so a stale wrong code (e.g. an old raw section
        // code from a pre-fix sync) gets overwritten with the friendly one.
        sqlx::query(
            "INSERT INTO courses (org_unit_id, name, code, semester, is_pinned, banner_url, last_accessed_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
             ON CONFLICT(org_unit_id) DO UPDATE SET
                name = excluded.name,
                code = excluded.code,
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

/// Mirror of the Ruby `Brilliant::Client#extract_course_code`: pull e.g.
/// "SWO-350", "SWO 370", "MAT101" from the full course title. Returns the
/// matched text trimmed (preserving the original separator the school used
/// — students recognize "SWO 370" specifically, not "SWO370").
fn extract_course_code(full_name: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?i)([A-Z]{2,4}\s*-?\s*\d{3,4})").ok()?;
    re.captures(full_name)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
}

/// Banner image lookup mirrors the Ruby `notification_service.rb`:
/// `OrgUnit.ImageUrl` → `OrgUnit.Image.ViewUrl` → `OrgUnit.Image.DisplayUrl`.
/// A relative path is rewritten with the configured Brightspace host so the
/// frontend can load it directly without needing a host-aware proxy.
fn extract_banner(org_unit: &Value, host: Option<&str>) -> Option<String> {
    let raw = org_unit.get("ImageUrl").and_then(|v| v.as_str())
        .or_else(|| org_unit.pointer("/Image/ViewUrl").and_then(|v| v.as_str()))
        .or_else(|| org_unit.pointer("/Image/DisplayUrl").and_then(|v| v.as_str()))?;
    if raw.is_empty() { return None; }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(raw.to_string());
    }
    let host = host?;
    Some(format!("https://{}{}", host, if raw.starts_with('/') { raw } else { &raw[..] }))
}
