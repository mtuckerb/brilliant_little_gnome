// Assignment sync. Pulls dropbox folders + quizzes and upserts into `assignments`.
// Honors `manually_edited` rows by preserving the locally-set due_date.

use crate::error::Result;
use crate::state::AppState;
use serde_json::Value;

pub async fn sync(state: &AppState, course_id: &str) -> Result<()> {
    // Dropbox folders.
    let folders = state.client.get_assignments(&state.pool, course_id, false).await.unwrap_or_default();
    for f in &folders {
        upsert_folder(state, course_id, f).await?;
    }

    // Quizzes (synthetic assignments prefixed with `quiz_`).
    let quizzes = state.client.get_quizzes(&state.pool, course_id, false).await.unwrap_or_default();
    for q in &quizzes {
        upsert_quiz(state, course_id, q).await?;
    }
    Ok(())
}

async fn upsert_folder(state: &AppState, course_id: &str, f: &Value) -> Result<()> {
    let Some(id_val) = f.get("Id").or_else(|| f.get("Identifier")) else { return Ok(()); };
    let id = id_val.as_i64().map(|n| n.to_string())
        .or_else(|| id_val.as_str().map(|s| s.to_string()))
        .ok_or_else(|| crate::error::AppError::Other("dropbox folder missing id".into()))?;

    let name = f.get("Name").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
    let due_date = f.get("DueDate").and_then(|v| v.as_str()).map(|s| s.to_string());
    let description = f.pointer("/CustomInstructions/Html").or_else(|| f.pointer("/CustomInstructions/Text"))
        .and_then(|v| v.as_str()).map(|s| s.to_string());
    let grade_item_id = f.get("GradeItemId").and_then(|v| v.as_i64()).map(|n| n.to_string());
    let is_graded = grade_item_id.is_some();

    sqlx::query(
        "INSERT INTO assignments (course_id, brightspace_id, name, due_date, description, is_graded, grade_item_id, assignment_type, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'dropbox', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(course_id, brightspace_id) DO UPDATE SET
            name = excluded.name,
            description = COALESCE(excluded.description, assignments.description),
            is_graded = excluded.is_graded,
            grade_item_id = COALESCE(excluded.grade_item_id, assignments.grade_item_id),
            -- Preserve user-edited due_date when manually_edited is set.
            due_date = CASE WHEN assignments.manually_edited = 1 THEN assignments.due_date ELSE excluded.due_date END,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(course_id)
    .bind(&id)
    .bind(&name)
    .bind(due_date)
    .bind(description)
    .bind(is_graded as i64)
    .bind(grade_item_id)
    .execute(&state.pool)
    .await?;

    // T-011: drain any pending assignment overlay queued during initial-pair.
    #[cfg(feature = "p2p")]
    {
        let key = format!("{}:{}", course_id, id);
        if let Err(e) = crate::p2p::bridge::drain_pending_overlay(
            &state.pool,
            crate::p2p::bridge::OverlayKind::Assignment,
            &key,
        )
        .await
        {
            tracing::warn!("drain pending assignment overlay {}: {}", key, e);
        }
    }

    Ok(())
}

async fn upsert_quiz(state: &AppState, course_id: &str, q: &Value) -> Result<()> {
    let Some(id_val) = q.get("QuizId").or_else(|| q.get("Id")) else { return Ok(()); };
    let raw_id = id_val.as_i64().map(|n| n.to_string())
        .or_else(|| id_val.as_str().map(|s| s.to_string()))
        .ok_or_else(|| crate::error::AppError::Other("quiz missing id".into()))?;
    let bs_id = format!("quiz_{}", raw_id);

    let name = q.get("Name").and_then(|v| v.as_str()).unwrap_or("Untitled Quiz").to_string();
    let due_date = q.get("DueDate").and_then(|v| v.as_str())
        .or_else(|| q.get("EndDate").and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    let description = q.pointer("/Description/Html").or_else(|| q.pointer("/Description/Text"))
        .and_then(|v| v.as_str()).map(|s| s.to_string());
    let grade_item_id = q.get("GradeItemId").and_then(|v| v.as_i64()).map(|n| n.to_string());

    sqlx::query(
        "INSERT INTO assignments (course_id, brightspace_id, name, due_date, description, is_graded, grade_item_id, assignment_type, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'quiz', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(course_id, brightspace_id) DO UPDATE SET
            name = excluded.name,
            description = COALESCE(excluded.description, assignments.description),
            grade_item_id = COALESCE(excluded.grade_item_id, assignments.grade_item_id),
            due_date = CASE WHEN assignments.manually_edited = 1 THEN assignments.due_date ELSE excluded.due_date END,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(course_id)
    .bind(&bs_id)
    .bind(&name)
    .bind(due_date)
    .bind(description)
    .bind((grade_item_id.is_some()) as i64)
    .bind(grade_item_id)
    .execute(&state.pool)
    .await?;

    // T-011: drain any pending assignment overlay for this quiz row.
    #[cfg(feature = "p2p")]
    {
        let key = format!("{}:{}", course_id, bs_id);
        if let Err(e) = crate::p2p::bridge::drain_pending_overlay(
            &state.pool,
            crate::p2p::bridge::OverlayKind::Assignment,
            &key,
        )
        .await
        {
            tracing::warn!("drain pending assignment overlay {}: {}", key, e);
        }
    }

    Ok(())
}
