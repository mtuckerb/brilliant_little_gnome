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
        let raw_name = ou.get("Name").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
        // Brightspace's `OrgUnit.Code` at this institution is a section/banner code
        // like "2620.UMS06-S.40963.1" — useless to students. We instead extract a
        // friendly code ("SWO-350", "PSY-220") from the course Name and fall back
        // to the raw Code only when extraction fails.
        //
        // The raw Brightspace name typically looks like
        //   "SWO 370:0001-Human Behav in the Socia En II (2026 Spring)"
        // We strip the code, the leading section number, and the trailing
        // "(YYYY Season)" so the saved name is just the human title:
        //   "Human Behav in the Socia En II"
        let semester = extract_semester(&raw_name);
        let (friendly_code, name) = extract_code_and_clean_name(&raw_name);
        let raw_code = ou.get("Code").and_then(|v| v.as_str()).map(|s| s.to_string());
        let code = friendly_code.or(raw_code);
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

        // T-011: a freshly-paired device may have a `course_overlays.<id>`
        // entry waiting on this row. Drain re-applies the overlay on
        // top so user intent (custom_color, target_grade, …) survives
        // the initial-pair → first-sync gap.
        #[cfg(feature = "p2p")]
        if let Err(e) = crate::p2p::bridge::drain_pending_overlay(
            &state.pool,
            crate::p2p::bridge::OverlayKind::Course,
            &id,
        )
        .await
        {
            tracing::warn!("drain pending course overlay {}: {}", id, e);
        }

        // T-017: `is_pinned` is Class-B (per §10). Brightspace
        // populates it from the user's PinDate, but the column is
        // user-overridable in principle, so we mirror the
        // sync-derived value into Loro. If a peer hasn't run
        // Brightspace sync yet, this gives them the right starting
        // point; if they have, the apply_remote path is idempotent.
        #[cfg(feature = "p2p")]
        if let Some(engine) = state.sync_engine() {
            if let Err(e) = engine
                .bridge()
                .apply_local(crate::p2p::bridge::LocalChange::Course {
                    id: id.clone(),
                    field: crate::p2p::doc::CourseField::IsPinned(is_pinned),
                })
                .await
            {
                tracing::warn!("apply_local sync is_pinned {}: {}", id, e);
            }
        }

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

/// Pull a friendly code (e.g. "SWO-370") out of the raw Brightspace course name
/// AND return the title with the code, the section delimiter, and the trailing
/// "(YYYY Season)" suffix all stripped — so callers don't need to render the
/// code twice.
///
/// Examples:
///   "SWO 370:0001-Human Behav in the Socia En II (2026 Spring)"
///     → (Some("SWO-370"), "Human Behav in the Socia En II")
///   "PSY-220 Introduction to Psychology"
///     → (Some("PSY-220"), "Introduction to Psychology")
///   "Special Topics in Robotics"            → (None, "Special Topics in Robotics")
///   "MAT101"                                → (Some("MAT-101"), "MAT101")  // fallback
fn extract_code_and_clean_name(full_name: &str) -> (Option<String>, String) {
    let code_re =
        regex::Regex::new(r"(?i)([A-Z]{2,4})\s*-?\s*(\d{3,4})").expect("static code regex");
    let m = code_re.captures(full_name);

    let friendly = m.as_ref().and_then(|c| {
        let prefix = c.get(1)?.as_str().to_uppercase();
        let number = c.get(2)?.as_str();
        Some(format!("{}-{}", prefix, number))
    });

    // Build the cleaned name: drop the matched code span, then strip a leading
    // section ("0001-", ":0001 ", etc.) and a trailing semester paren.
    let mut cleaned = match m.as_ref().and_then(|c| c.get(0)) {
        Some(span) => {
            let mut s = String::with_capacity(full_name.len());
            s.push_str(&full_name[..span.start()]);
            s.push_str(&full_name[span.end()..]);
            s
        }
        None => full_name.to_string(),
    };

    let sem_re = regex::Regex::new(
        r"(?i)\s*\(\s*(?:\d{4}\s+(?:Spring|Fall|Summer|Winter|Session|Quarter)|(?:Spring|Fall|Summer|Winter|Session|Quarter)\s+\d{4})\s*\)\s*$",
    ).expect("static semester regex");
    cleaned = sem_re.replace(&cleaned, "").to_string();

    let section_re = regex::Regex::new(r"^[\s:\-\.]*\d{2,4}[\s:\-\.]+").expect("static section regex");
    cleaned = section_re.replace(&cleaned, "").to_string();

    cleaned = cleaned
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '-' | '_' | '.'))
        .to_string();

    // If cleanup ate everything (e.g. the whole name was just "MAT101"), fall
    // back to the raw name so we never store empty.
    let final_name = if cleaned.is_empty() {
        full_name.trim().to_string()
    } else {
        cleaned
    };

    (friendly, final_name)
}

#[cfg(test)]
mod tests {
    use super::extract_code_and_clean_name;

    #[test]
    fn cleans_typical_brightspace_name() {
        let (code, name) =
            extract_code_and_clean_name("SWO 370:0001-Human Behav in the Socia En II (2026 Spring)");
        assert_eq!(code.as_deref(), Some("SWO-370"));
        assert_eq!(name, "Human Behav in the Socia En II");
    }

    #[test]
    fn handles_already_hyphenated_code() {
        let (code, name) = extract_code_and_clean_name("PSY-220 Introduction to Psychology");
        assert_eq!(code.as_deref(), Some("PSY-220"));
        assert_eq!(name, "Introduction to Psychology");
    }

    #[test]
    fn falls_back_when_no_code() {
        let (code, name) = extract_code_and_clean_name("Special Topics in Robotics");
        assert_eq!(code, None);
        assert_eq!(name, "Special Topics in Robotics");
    }

    #[test]
    fn falls_back_when_name_is_only_a_code() {
        let (code, name) = extract_code_and_clean_name("MAT101");
        assert_eq!(code.as_deref(), Some("MAT-101"));
        assert_eq!(name, "MAT101");
    }
}

/// Banner image lookup mirrors the Ruby `notification_service.rb` and covers
/// the image URL variants Brightspace has returned from enrollment/course-tile
/// payloads. A relative path is rewritten with the configured Brightspace host.
fn extract_banner(org_unit: &Value, host: Option<&str>) -> Option<String> {
    let raw = [
        "/ImageUrl",
        "/Image/Url",
        "/Image/ViewUrl",
        "/Image/DisplayUrl",
        "/Image/LargeUrl",
        "/Image/MediumUrl",
        "/Image/SmallUrl",
        "/Image/TileUrl",
        "/Image/ThumbnailUrl",
        "/Image/BannerUrl",
        "/Image/BannerImageUrl",
        "/Image/CardImageUrl",
    ]
    .iter()
    .filter_map(|path| org_unit.pointer(path).and_then(|v| v.as_str()))
    .find(|url| !url.is_empty())?;

    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(raw.to_string());
    }
    let host = host?;
    Some(format!("https://{}{}", host, if raw.starts_with('/') { raw } else { &raw[..] }))
}
