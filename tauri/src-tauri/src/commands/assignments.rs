use super::AppStateArg;
use crate::error::{AppError, Result};
use crate::models::Assignment;
use rand::RngCore;

#[tauri::command]
pub async fn list_assignments(state: AppStateArg<'_>, course_id: Option<String>) -> Result<Vec<Assignment>> {
    let rows = if let Some(cid) = course_id {
        sqlx::query_as::<_, Assignment>(
            "SELECT id, course_id, brightspace_id, name, due_date, description, is_graded, grade_item_id, assignment_type, completed, completed_at, synthetic, optional, external_url, manually_edited FROM assignments WHERE course_id = ? ORDER BY due_date ASC NULLS LAST",
        )
        .bind(cid)
        .fetch_all(&state.pool).await?
    } else {
        sqlx::query_as::<_, Assignment>(
            "SELECT id, course_id, brightspace_id, name, due_date, description, is_graded, grade_item_id, assignment_type, completed, completed_at, synthetic, optional, external_url, manually_edited FROM assignments WHERE completed = 0 ORDER BY due_date ASC NULLS LAST",
        )
        .fetch_all(&state.pool).await?
    };
    Ok(rows)
}

#[tauri::command]
pub async fn toggle_assignment_complete(state: AppStateArg<'_>, id: i64) -> Result<()> {
    sqlx::query("UPDATE assignments SET completed = 1 - completed, completed_at = CASE WHEN completed = 0 THEN CURRENT_TIMESTAMP ELSE NULL END, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id).execute(&state.pool).await?;
    #[cfg(feature = "p2p")]
    if let Some((key, completed, completed_at_unix)) =
        read_assignment_completion(&state, id).await
    {
        push_assignment_field(
            &state,
            key.clone(),
            crate::p2p::doc::AssignmentField::Completed(completed),
        )
        .await;
        push_assignment_field(
            &state,
            key,
            crate::p2p::doc::AssignmentField::CompletedAt(completed_at_unix),
        )
        .await;
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_assignment_optional(state: AppStateArg<'_>, id: i64) -> Result<()> {
    sqlx::query("UPDATE assignments SET optional = 1 - optional, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id).execute(&state.pool).await?;
    #[cfg(feature = "p2p")]
    if let Some((key, optional)) = read_assignment_bool(&state, id, "optional").await {
        push_assignment_field(
            &state,
            key,
            crate::p2p::doc::AssignmentField::Optional(optional),
        )
        .await;
    }
    Ok(())
}

#[tauri::command]
pub async fn update_assignment_due_date(state: AppStateArg<'_>, id: i64, due_date: Option<String>) -> Result<()> {
    sqlx::query("UPDATE assignments SET due_date = ?, manually_edited = 1, manually_edited_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(&due_date).bind(id).execute(&state.pool).await?;
    #[cfg(feature = "p2p")]
    if let Some(key) = read_assignment_key(&state, id).await {
        // ManuallyEdited + the timestamp accompany every due-date edit
        // — the existing Brightspace-protect-overrides logic is keyed
        // on those two fields, so the peer needs them to make the
        // same protection decision.
        let now = chrono::Utc::now().timestamp();
        push_assignment_field(
            &state,
            key.clone(),
            crate::p2p::doc::AssignmentField::DueDate(due_date),
        )
        .await;
        push_assignment_field(
            &state,
            key.clone(),
            crate::p2p::doc::AssignmentField::ManuallyEdited(true),
        )
        .await;
        push_assignment_field(
            &state,
            key,
            crate::p2p::doc::AssignmentField::ManuallyEditedAt(Some(now)),
        )
        .await;
    }
    Ok(())
}

/// Edit the user-facing fields of any assignment — Brightspace-sourced or
/// synthetic. Marks the row manually_edited so the Brightspace sync preserves
/// the edited name/description/due_date instead of overwriting them.
#[tauri::command]
pub async fn update_assignment(
    state: AppStateArg<'_>,
    id: i64,
    name: String,
    due_date: Option<String>,
    description: Option<String>,
    external_url: Option<String>,
) -> Result<Assignment> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Other("name cannot be empty".to_string()));
    }
    let row: Assignment = sqlx::query_as::<_, Assignment>(
        "UPDATE assignments SET name = ?, due_date = ?, description = ?, external_url = ?, manually_edited = 1, manually_edited_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?
         RETURNING id, course_id, brightspace_id, name, due_date, description, is_graded, grade_item_id, assignment_type, completed, completed_at, synthetic, optional, external_url, manually_edited",
    )
    .bind(trimmed)
    .bind(due_date.as_deref())
    .bind(description.as_deref())
    .bind(external_url.as_deref())
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::Other(format!("assignment {} not found", id)))?;

    #[cfg(feature = "p2p")]
    if row.synthetic {
        // Synthetic rows live in the Loro synthetic_assignments map — refresh
        // the whole value like create_synthetic_assignment does, so a freshly
        // paired device materializes the edited fields too.
        use crate::p2p::bridge::LocalChange;
        use crate::p2p::doc::SyntheticAssignment;
        if let Some(engine) = state.sync_engine() {
            let created_at = read_assignment_created_at(&state, id)
                .await
                .unwrap_or_else(|| chrono::Utc::now().timestamp());
            let value = SyntheticAssignment {
                course_id: row.course_id.parse::<i64>().unwrap_or(0),
                name: row.name.clone(),
                description: row.description.clone().unwrap_or_default(),
                due_date: row.due_date.clone(),
                optional: row.optional,
                completed: row.completed,
                completed_at: row.completed_at.as_deref().and_then(sql_timestamp_to_unix),
                created_at,
            };
            if let Err(e) = engine
                .bridge()
                .apply_local(LocalChange::SyntheticAssignmentUpsert {
                    uuid: row.brightspace_id.clone(),
                    value,
                })
                .await
            {
                tracing::warn!("apply_local synthetic upsert {}: {}", row.brightspace_id, e);
            }
        }
    } else {
        let key = format!("{}:{}", row.course_id, row.brightspace_id);
        let now = chrono::Utc::now().timestamp();
        push_assignment_field(
            &state,
            key.clone(),
            crate::p2p::doc::AssignmentField::Name(row.name.clone()),
        )
        .await;
        push_assignment_field(
            &state,
            key.clone(),
            crate::p2p::doc::AssignmentField::Description(row.description.clone().unwrap_or_default()),
        )
        .await;
        push_assignment_field(
            &state,
            key.clone(),
            crate::p2p::doc::AssignmentField::DueDate(row.due_date.clone()),
        )
        .await;
        push_assignment_field(
            &state,
            key.clone(),
            crate::p2p::doc::AssignmentField::ManuallyEdited(true),
        )
        .await;
        push_assignment_field(
            &state,
            key,
            crate::p2p::doc::AssignmentField::ManuallyEditedAt(Some(now)),
        )
        .await;
    }

    Ok(row)
}

/// Create a user-defined ("synthetic") assignment that doesn't exist in
/// Brightspace. Mirrors the original Sinatra app's `syn_<hex>` ID convention so
/// downstream sync can recognize them and never overwrite them from the API.
#[tauri::command]
pub async fn create_synthetic_assignment(
    state: AppStateArg<'_>,
    course_id: String,
    name: String,
    due_date: Option<String>,
    description: Option<String>,
) -> Result<Assignment> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Other("name cannot be empty".to_string()));
    }
    let bid = synthetic_id();
    let row: Assignment = sqlx::query_as::<_, Assignment>(
        "INSERT INTO assignments (course_id, brightspace_id, name, due_date, description, is_graded, assignment_type, completed, synthetic, optional, manually_edited, manually_edited_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 0, 'synthetic', 0, 1, 0, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         RETURNING id, course_id, brightspace_id, name, due_date, description, is_graded, grade_item_id, assignment_type, completed, completed_at, synthetic, optional, external_url, manually_edited",
    )
    .bind(&course_id)
    .bind(&bid)
    .bind(trimmed)
    .bind(due_date.as_deref())
    .bind(description.as_deref())
    .fetch_one(&state.pool)
    .await?;

    // T-016: synthetic assignments live entirely in Loro (no
    // Brightspace counterpart). The bridge knows to mirror these
    // back into the `assignments` table on the receiving device with
    // synthetic = 1.
    #[cfg(feature = "p2p")]
    {
        use crate::p2p::bridge::LocalChange;
        use crate::p2p::doc::SyntheticAssignment;
        if let Some(engine) = state.sync_engine() {
            // courses.org_unit_id is TEXT; the Loro view stores the
            // numeric variant. Falling back to 0 on parse failure
            // keeps a non-numeric Brightspace id (rare but possible
            // for some institutions) from blocking the local INSERT.
            let course_num = course_id.parse::<i64>().unwrap_or(0);
            let value = SyntheticAssignment {
                course_id: course_num,
                name: trimmed.to_string(),
                description: description.unwrap_or_default(),
                due_date,
                optional: false,
                completed: false,
                completed_at: None,
                created_at: chrono::Utc::now().timestamp(),
            };
            if let Err(e) = engine
                .bridge()
                .apply_local(LocalChange::SyntheticAssignmentUpsert {
                    uuid: bid.clone(),
                    value,
                })
                .await
            {
                tracing::warn!("apply_local synthetic upsert {}: {}", bid, e);
            }
        }
    }

    Ok(row)
}

/// Hard-delete an assignment row. Synthetic-only — refuse for API-sourced rows
/// because the next sync would just resurrect them, which would surprise users.
#[tauri::command]
pub async fn delete_assignment(state: AppStateArg<'_>, id: i64) -> Result<()> {
    let row: Option<(i64, Option<String>)> = sqlx::query_as(
        "SELECT synthetic, brightspace_id FROM assignments WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    // `brightspace_id` is only used by the p2p mirror block below.
    // Without the feature it's unused, but pulling it from the
    // SELECT is the same query either way; underscore suppresses
    // the no-features warning.
    let Some((synthetic, _brightspace_id)) = row else {
        return Err(AppError::Other(format!("assignment {} not found", id)));
    };
    if synthetic == 0 {
        return Err(AppError::Other(
            "only synthetic assignments can be deleted; toggle complete instead".to_string(),
        ));
    }
    sqlx::query("DELETE FROM assignments WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;

    // T-016: tell peers the synthetic is gone.
    #[cfg(feature = "p2p")]
    if let Some(uuid) = _brightspace_id {
        if let Some(engine) = state.sync_engine() {
            if let Err(e) = engine
                .bridge()
                .apply_local(crate::p2p::bridge::LocalChange::SyntheticAssignmentDelete {
                    uuid: uuid.clone(),
                })
                .await
            {
                tracing::warn!("apply_local synthetic delete {}: {}", uuid, e);
            }
        }
    }

    Ok(())
}

fn synthetic_id() -> String {
    let mut buf = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut buf);
    let hex: String = buf.iter().map(|b| format!("{:02x}", b)).collect();
    format!("syn_{}", hex)
}

// ---- p2p sync helpers (feature-gated) ------------------------------------

#[cfg(feature = "p2p")]
async fn read_assignment_key(state: &AppStateArg<'_>, id: i64) -> Option<String> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT course_id, brightspace_id FROM assignments WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let (course_id, bid) = row?;
    Some(format!("{course_id}:{bid}"))
}

#[cfg(feature = "p2p")]
async fn read_assignment_bool(
    state: &AppStateArg<'_>,
    id: i64,
    col: &str,
) -> Option<(String, bool)> {
    let q = format!(
        "SELECT course_id, brightspace_id, {col} FROM assignments WHERE id = ?"
    );
    let row: Option<(String, String, i64)> =
        sqlx::query_as(&q).bind(id).fetch_optional(&state.pool).await.ok().flatten();
    let (course_id, bid, val) = row?;
    Some((format!("{course_id}:{bid}"), val != 0))
}

/// Read the freshly-toggled completion state plus the matching
/// `completed_at` value (NULL when uncompleted, current timestamp
/// when newly completed). The Loro view stores `completed_at` as
/// unix seconds — convert from the SQL TEXT timestamp here so the
/// peer receives a stable integer.
#[cfg(feature = "p2p")]
async fn read_assignment_completion(
    state: &AppStateArg<'_>,
    id: i64,
) -> Option<(String, bool, Option<i64>)> {
    let row: Option<(String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT course_id, brightspace_id, completed, completed_at FROM assignments WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let (course_id, bid, completed, completed_at_text) = row?;
    let completed_at_unix = completed_at_text.as_deref().and_then(sql_timestamp_to_unix);
    Some((format!("{course_id}:{bid}"), completed != 0, completed_at_unix))
}

/// SQLite CURRENT_TIMESTAMP comes back as 'YYYY-MM-DD HH:MM:SS'
/// (no timezone — implicit UTC). Try the RFC3339 form first
/// for forward compatibility; fall back to the naive form.
#[cfg(feature = "p2p")]
fn sql_timestamp_to_unix(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp())
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|n| n.and_utc().timestamp())
        })
}

#[cfg(feature = "p2p")]
async fn read_assignment_created_at(state: &AppStateArg<'_>, id: i64) -> Option<i64> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT created_at FROM assignments WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    row.and_then(|(s,)| s).as_deref().and_then(sql_timestamp_to_unix)
}

#[cfg(feature = "p2p")]
async fn push_assignment_field(
    state: &AppStateArg<'_>,
    key: String,
    field: crate::p2p::doc::AssignmentField,
) {
    if let Some(engine) = state.sync_engine() {
        if let Err(e) = engine
            .bridge()
            .apply_local(crate::p2p::bridge::LocalChange::Assignment {
                key: key.clone(),
                field,
            })
            .await
        {
            tracing::warn!("apply_local assignment {}: {}", key, e);
        }
    }
}
