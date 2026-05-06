use super::AppStateArg;
use crate::error::Result;
use crate::models::{Grade, GradeRow, GradeStats};
use serde::Serialize;

#[derive(Serialize)]
pub struct GradesSummaryResponse {
    pub rows: Vec<GradeRow>,
    pub stats: GradeStats,
}

#[tauri::command]
pub async fn grades_summary(state: AppStateArg<'_>, course_id: String) -> Result<GradesSummaryResponse> {
    let course: Option<(Option<f64>,)> = sqlx::query_as("SELECT target_grade FROM courses WHERE org_unit_id = ?")
        .bind(&course_id)
        .fetch_optional(&state.pool)
        .await?;
    let target = course.and_then(|c| c.0).unwrap_or(93.0);

    let grades: Vec<Grade> = sqlx::query_as(
        "SELECT id, course_id, brightspace_id, name, displayed_grade, numerator, denominator, weight, due_date, is_extra_credit, hidden, manually_marked_ungraded, expected_score, comments FROM grades WHERE course_id = ? ORDER BY due_date ASC NULLS LAST, name ASC",
    )
    .bind(&course_id)
    .fetch_all(&state.pool)
    .await?;

    // Compute summary. Mirrors the Ruby logic: hidden rows excluded; ungraded skipped from
    // earned/possible; expected_score injects a synthetic numerator for ungraded items.
    let mut earned = 0.0;
    let mut possible = 0.0;
    let mut all_possible = 0.0;
    let mut rows: Vec<GradeRow> = Vec::with_capacity(grades.len());
    for g in &grades {
        if g.hidden { continue; }
        let denom = g.denominator.unwrap_or(0.0);
        all_possible += denom;
        let is_graded = !g.manually_marked_ungraded && g.numerator.is_some() && denom > 0.0;
        let is_expected = !is_graded && g.expected_score.is_some() && denom > 0.0;

        let perc = if is_graded {
            Some((g.numerator.unwrap_or(0.0) / denom) * 100.0)
        } else if is_expected {
            g.expected_score
        } else { None };

        if is_graded {
            earned += g.numerator.unwrap_or(0.0);
            possible += denom;
        } else if is_expected {
            earned += denom * (g.expected_score.unwrap_or(0.0) / 100.0);
            possible += denom;
        }

        rows.push(GradeRow {
            grade: g.clone(),
            perc,
            is_graded,
            is_expected,
            rel_weight: if all_possible > 0.0 { denom / all_possible.max(1.0) } else { 0.0 },
            submitted: None,
            submitted_at: None,
        });
    }

    let score = if possible > 0.0 { Some(earned / possible * 100.0) } else { None };
    let remaining = (all_possible - possible).max(0.0);
    let needed_total = all_possible * (target / 100.0);
    let required_avg = if remaining > 0.0 {
        Some(((needed_total - earned) / remaining * 100.0).max(0.0))
    } else { None };
    let is_impossible = required_avg.map(|r| r > 100.0).unwrap_or(false);

    Ok(GradesSummaryResponse {
        rows,
        stats: GradeStats {
            score,
            confidence: if all_possible > 0.0 { possible / all_possible } else { 0.0 },
            total_points_earned: earned,
            total_points_possible: possible,
            all_possible_points: all_possible,
            remaining_points: remaining,
            target_grade: target,
            required_avg,
            is_impossible,
        },
    })
}

#[tauri::command]
pub async fn toggle_grade_hidden(state: AppStateArg<'_>, id: i64) -> Result<()> {
    sqlx::query("UPDATE grades SET hidden = 1 - hidden, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id).execute(&state.pool).await?;
    #[cfg(feature = "p2p")]
    if let Some((key, hidden)) = read_grade_key_and_bool(&state, id, "hidden").await {
        push_grade_field(&state, key, crate::p2p::doc::GradeField::Hidden(hidden)).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_grade_ungraded(state: AppStateArg<'_>, id: i64) -> Result<()> {
    sqlx::query("UPDATE grades SET manually_marked_ungraded = 1 - manually_marked_ungraded, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id).execute(&state.pool).await?;
    #[cfg(feature = "p2p")]
    if let Some((key, marked)) =
        read_grade_key_and_bool(&state, id, "manually_marked_ungraded").await
    {
        push_grade_field(
            &state,
            key,
            crate::p2p::doc::GradeField::ManuallyMarkedUngraded(marked),
        )
        .await;
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_grade_extra_credit(state: AppStateArg<'_>, id: i64) -> Result<()> {
    sqlx::query("UPDATE grades SET is_extra_credit = 1 - is_extra_credit, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id).execute(&state.pool).await?;
    #[cfg(feature = "p2p")]
    if let Some((key, extra)) = read_grade_key_and_bool(&state, id, "is_extra_credit").await {
        push_grade_field(
            &state,
            key,
            crate::p2p::doc::GradeField::IsExtraCredit(extra),
        )
        .await;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_expected_score(state: AppStateArg<'_>, id: i64, expected: Option<f64>) -> Result<()> {
    sqlx::query("UPDATE grades SET expected_score = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(expected).bind(id).execute(&state.pool).await?;
    #[cfg(feature = "p2p")]
    if let Some(key) = read_grade_key(&state, id).await {
        push_grade_field(
            &state,
            key,
            crate::p2p::doc::GradeField::ExpectedScore(expected),
        )
        .await;
    }
    Ok(())
}

// ---- p2p sync helpers (feature-gated) ------------------------------------

/// Read back the freshly-written value of a single boolean column on
/// `grades` so we can mirror the canonical state into Loro. Toggles
/// flip via `1 - col`, so the caller can't compute the new value from
/// the input — the DB read is the source of truth.
#[cfg(feature = "p2p")]
async fn read_grade_key_and_bool(
    state: &AppStateArg<'_>,
    id: i64,
    col: &str,
) -> Option<(String, bool)> {
    // Build the column name into the query string — sqlx doesn't bind
    // identifiers, but `col` is a hard-coded string from each command,
    // so there's no injection surface.
    let q = format!(
        "SELECT course_id, brightspace_id, {col} FROM grades WHERE id = ?"
    );
    let row: Option<(String, Option<String>, i64)> =
        sqlx::query_as(&q).bind(id).fetch_optional(&state.pool).await.ok().flatten();
    let (course_id, bid, val) = row?;
    let bid = bid?;
    Some((format!("{course_id}:{bid}"), val != 0))
}

/// Same as `read_grade_key_and_bool` but only fetches the composite
/// key — used when the new value comes from the command's own input
/// (e.g. set_expected_score).
#[cfg(feature = "p2p")]
async fn read_grade_key(state: &AppStateArg<'_>, id: i64) -> Option<String> {
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT course_id, brightspace_id FROM grades WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let (course_id, bid) = row?;
    Some(format!("{course_id}:{}", bid?))
}

#[cfg(feature = "p2p")]
async fn push_grade_field(
    state: &AppStateArg<'_>,
    key: String,
    field: crate::p2p::doc::GradeField,
) {
    if let Some(engine) = state.sync_engine() {
        if let Err(e) = engine
            .bridge()
            .apply_local(crate::p2p::bridge::LocalChange::Grade { key: key.clone(), field })
            .await
        {
            tracing::warn!("apply_local grade {}: {}", key, e);
        }
    }
}
