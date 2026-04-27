// Grade sync. Mirrors `Brilliant::Sync::GradeService`:
//   1. Fetch grade values (`/grades/values/myGradeValues/`) and definitions
//      (`/grades/`).
//   2. Merge by Brightspace grade-object identifier.
//   3. Upsert into `grades`, preserving user-edited columns (`hidden`,
//      `manually_marked_ungraded`, `expected_score`).
//   4. Clear `manually_marked_ungraded` when a real positive numerator arrives.

use crate::error::Result;
use crate::state::AppState;
use serde_json::Value;
use std::collections::HashMap;

pub async fn sync(state: &AppState, course_id: &str) -> Result<()> {
    let values = state.client.get_grades(&state.pool, course_id, false).await.unwrap_or_default();
    let defs = state.client.get_grade_definitions(&state.pool, course_id, false).await.unwrap_or_default();

    let mut values_map: HashMap<String, &Value> = HashMap::new();
    for v in &values {
        if let Some(id) = pick_id(v, &["GradeObjectIdentifier", "Identifier"]) {
            values_map.insert(id, v);
        }
    }
    let mut defs_map: HashMap<String, &Value> = HashMap::new();
    for d in &defs {
        if let Some(id) = pick_id(d, &["Id", "Identifier"]) {
            defs_map.insert(id, d);
        }
    }

    let mut all_ids: Vec<String> = defs_map.keys().chain(values_map.keys()).cloned().collect();
    all_ids.sort();
    all_ids.dedup();

    for obj_id in all_ids {
        let defn = defs_map.get(&obj_id).copied();
        let val = values_map.get(&obj_id).copied();

        let name = defn.and_then(|d| d.get("Name").and_then(|v| v.as_str()))
            .or_else(|| val.and_then(|v| v.get("GradeObjectName").and_then(|x| x.as_str())))
            .or_else(|| val.and_then(|v| v.get("Name").and_then(|x| x.as_str())))
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Grade Item {}", obj_id));

        let denominator = val.and_then(|v| pick_f64(v, &["GradeValue/PointsDenominator", "GradeValue/Denominator", "PointsDenominator", "Denominator"]))
            .or_else(|| defn.and_then(|d| pick_f64(d, &["MaxPoints"])));
        let numerator = val.and_then(|v| pick_f64(v, &["GradeValue/PointsNumerator", "GradeValue/Numerator", "PointsNumerator", "Numerator"]));
        let weight = defn.and_then(|d| pick_f64(d, &["Weight"]))
            .or_else(|| val.and_then(|v| pick_f64(v, &["Weight", "WeightAchieved", "GradeValue/Weight"])));
        let displayed = val.and_then(|v| v.get("DisplayedGrade").and_then(|x| x.as_str())).map(|s| s.to_string());
        let comments = extract_comments(val);
        let is_extra = defn
            .and_then(|d| d.get("IsExtraCredit").and_then(|x| x.as_bool()).or_else(|| d.get("IsBonus").and_then(|x| x.as_bool())))
            .unwrap_or(false);
        let grade_type = defn.and_then(|d| d.get("GradeType").and_then(|v| v.as_str())).map(|s| s.to_string())
            .or_else(|| val.and_then(|v| v.get("GradeObjectTypeName").and_then(|x| x.as_str())).map(|s| s.to_string()));

        // Upsert preserving user edits.
        sqlx::query(
            "INSERT INTO grades (course_id, brightspace_id, name, displayed_grade, numerator, denominator, weight, is_extra_credit, comments, grade_object_type, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
             ON CONFLICT(course_id, brightspace_id) DO UPDATE SET
                name = excluded.name,
                displayed_grade = excluded.displayed_grade,
                numerator = excluded.numerator,
                denominator = excluded.denominator,
                weight = excluded.weight,
                is_extra_credit = excluded.is_extra_credit,
                comments = COALESCE(excluded.comments, grades.comments),
                grade_object_type = COALESCE(excluded.grade_object_type, grades.grade_object_type),
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind(course_id)
        .bind(&obj_id)
        .bind(&name)
        .bind(displayed)
        .bind(numerator)
        .bind(denominator)
        .bind(weight)
        .bind(is_extra as i64)
        .bind(comments)
        .bind(grade_type)
        .execute(&state.pool)
        .await?;
    }

    // Clear manually_marked_ungraded if a positive numerator now exists.
    sqlx::query(
        "UPDATE grades SET manually_marked_ungraded = 0, updated_at = CURRENT_TIMESTAMP
         WHERE course_id = ? AND manually_marked_ungraded = 1 AND numerator IS NOT NULL AND numerator > 0",
    )
    .bind(course_id)
    .execute(&state.pool)
    .await?;

    Ok(())
}

fn pick_id(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(x) = v.get(*k) {
            if let Some(s) = x.as_str() { return Some(s.to_string()); }
            if let Some(n) = x.as_i64() { return Some(n.to_string()); }
            if let Some(n) = x.as_f64() { return Some((n as i64).to_string()); }
        }
    }
    None
}

fn pick_f64(v: &Value, paths: &[&str]) -> Option<f64> {
    for p in paths {
        let mut cur = v;
        let mut ok = true;
        for seg in p.split('/') {
            match cur.get(seg) {
                Some(next) => cur = next,
                None => { ok = false; break; }
            }
        }
        if ok {
            if let Some(n) = cur.as_f64() { return Some(n); }
            if let Some(s) = cur.as_str() {
                if let Ok(n) = s.parse::<f64>() { return Some(n); }
            }
        }
    }
    None
}

fn extract_comments(val: Option<&Value>) -> Option<String> {
    let v = val?;
    let candidates = ["Comments", "GradeValue/Comments", "PrivateComments", "GradeValue/PrivateComments"];
    for path in &candidates {
        let mut cur = v;
        let mut ok = true;
        for seg in path.split('/') {
            match cur.get(seg) {
                Some(next) => cur = next,
                None => { ok = false; break; }
            }
        }
        if !ok { continue; }
        if let Some(s) = cur.as_str() {
            if !s.is_empty() { return Some(s.to_string()); }
        }
        if let Some(html) = cur.get("Html").and_then(|x| x.as_str()) {
            if !html.is_empty() { return Some(html.to_string()); }
        }
        if let Some(text) = cur.get("Text").and_then(|x| x.as_str()) {
            if !text.is_empty() { return Some(text.to_string()); }
        }
    }
    None
}
