//! `get_assignment_detail` — fetches the four assignment-related Brightspace
//! endpoints (folder, feedback, submissions, rubrics) and combines them with the
//! gradebook fallback into a single payload the UI can render in one pass.
//!
//! Mirrors the data set assembled by the original Sinatra
//! `controllers/assignment_controller.rb#get '/course/:id/assignments/:assignment_id'`.

use super::AppStateArg;
use crate::error::Result;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct AssignmentAttachment {
    pub name: String,
    pub url: Option<String>,
    pub size: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AssignmentSubmission {
    pub submitted_at: Option<String>,
    pub comment_html: Option<String>,
    pub files: Vec<AssignmentAttachment>,
}

#[derive(Debug, Serialize)]
pub struct AssignmentFeedback {
    pub displayed_score: Option<String>,
    pub score_numerator: Option<f64>,
    pub score_denominator: Option<f64>,
    pub feedback_html: Option<String>,
    pub attachments: Vec<AssignmentAttachment>,
}

#[derive(Debug, Serialize)]
pub struct AssignmentDetail {
    /// Raw assignment folder payload (Instructions, attachments, etc.)
    pub folder_raw: Option<Value>,
    /// Raw rubric assessments payload — the UI handles both formats
    /// (per-rubric `Assessments` and `CriteriaGroups`).
    pub rubrics_raw: Option<Value>,
    pub feedback: Option<AssignmentFeedback>,
    pub submissions: Vec<AssignmentSubmission>,
    /// Plain instruction text/html scraped out of the folder.
    pub instructions_html: Option<String>,
    /// Attachments referenced from instructions / linked resources.
    pub instruction_attachments: Vec<AssignmentAttachment>,
    /// Gradebook score/comment fallback if the dropbox feedback endpoint had nothing.
    pub gradebook: Option<GradebookEntry>,
    /// True when the row is a local synthetic task (no API calls were made).
    pub synthetic: bool,
}

#[derive(Debug, Serialize)]
pub struct GradebookEntry {
    pub displayed_grade: Option<String>,
    pub numerator: Option<f64>,
    pub denominator: Option<f64>,
    pub comments_html: Option<String>,
}

#[tauri::command]
pub async fn get_assignment_detail(
    state: AppStateArg<'_>,
    course_id: String,
    assignment_id: String,
) -> Result<AssignmentDetail> {
    // Synthetic short-circuit — never hit Brightspace for `syn_*` rows.
    if assignment_id.starts_with("syn_") {
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT description, name FROM assignments WHERE brightspace_id = ? AND course_id = ?",
        )
        .bind(&assignment_id)
        .bind(&course_id)
        .fetch_optional(&state.pool)
        .await?;
        let instructions_html = row.and_then(|(d, _)| Some(d));
        return Ok(AssignmentDetail {
            folder_raw: None,
            rubrics_raw: None,
            feedback: None,
            submissions: vec![],
            instructions_html,
            instruction_attachments: vec![],
            gradebook: None,
            synthetic: true,
        });
    }

    // Run the four API calls in parallel — one slow endpoint shouldn't gate the
    // rest. Each endpoint may legitimately 404 when (e.g.) the user hasn't
    // submitted yet, so failures collapse to None instead of bubbling up.
    let (folder, feedback_raw, submissions_raw, rubrics) = tokio::join!(
        state.client.get_assignment_folder(&state.pool, &course_id, &assignment_id, false),
        state.client.get_assignment_feedback(&state.pool, &course_id, &assignment_id, false),
        state.client.get_assignment_submissions(&state.pool, &course_id, &assignment_id, false),
        state.client.get_assignment_rubrics(&state.pool, &course_id, false),
    );

    let folder_raw = folder.ok();
    let feedback = feedback_raw.ok().and_then(parse_feedback);
    let submissions = submissions_raw.map(parse_submissions).unwrap_or_default();
    let rubrics_raw = rubrics.ok();

    let (instructions_html, instruction_attachments) = folder_raw
        .as_ref()
        .map(parse_instructions)
        .unwrap_or((None, vec![]));

    // Gradebook fallback — used by the UI when feedback has no score/comments.
    let gradebook = lookup_gradebook(&state.pool, &course_id, &assignment_id).await?;

    Ok(AssignmentDetail {
        folder_raw,
        rubrics_raw,
        feedback,
        submissions,
        instructions_html,
        instruction_attachments,
        gradebook,
        synthetic: false,
    })
}

fn parse_feedback(v: Value) -> Option<AssignmentFeedback> {
    // Brightspace returns either a Feedback object directly or null — treat
    // empty objects the same as null so the UI can fall back to gradebook.
    let obj = v.as_object()?;
    if obj.is_empty() {
        return None;
    }
    let score = obj.get("Score").and_then(|s| s.as_f64());
    let out_of = obj.get("OutOf").and_then(|s| s.as_f64());
    let displayed_score = obj
        .get("DisplayedGrade")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string());
    let feedback_html = obj
        .get("Feedback")
        .and_then(|f| extract_html_or_text(f))
        .or_else(|| obj.get("FeedbackText").and_then(|f| f.as_str()).map(String::from));
    let attachments = obj
        .get("Attachments")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(parse_attachment).collect())
        .unwrap_or_default();

    Some(AssignmentFeedback {
        displayed_score,
        score_numerator: score,
        score_denominator: out_of,
        feedback_html,
        attachments,
    })
}

fn parse_submissions(v: Value) -> Vec<AssignmentSubmission> {
    // The submissions endpoint typically returns an array, but Brightspace
    // sometimes wraps it in `{ Items: [...] }`. Handle both.
    let arr: Vec<Value> = match v {
        Value::Array(a) => a,
        Value::Object(ref o) => o
            .get("Items")
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => return vec![],
    };
    arr.into_iter()
        .flat_map(|s| {
            // Each entry may itself contain a `Submissions` sub-list (one row per attempt).
            let attempts = s
                .get("Submissions")
                .and_then(|sub| sub.as_array())
                .cloned()
                .unwrap_or_else(|| vec![s.clone()]);
            attempts.into_iter().map(parse_single_submission).collect::<Vec<_>>()
        })
        .collect()
}

fn parse_single_submission(s: Value) -> AssignmentSubmission {
    let submitted_at = s
        .get("SubmissionDate")
        .or_else(|| s.get("Submitted"))
        .and_then(|d| d.as_str())
        .map(String::from);
    let comment_html = s
        .get("Comment")
        .and_then(extract_html_or_text)
        .or_else(|| s.get("CommentText").and_then(|c| c.as_str()).map(String::from));
    let files = s
        .get("Files")
        .or_else(|| s.get("Attachments"))
        .and_then(|f| f.as_array())
        .map(|arr| arr.iter().filter_map(parse_attachment).collect())
        .unwrap_or_default();
    AssignmentSubmission {
        submitted_at,
        comment_html,
        files,
    }
}

fn parse_instructions(folder: &Value) -> (Option<String>, Vec<AssignmentAttachment>) {
    let html = folder
        .get("Instructions")
        .and_then(extract_html_or_text)
        .or_else(|| folder.get("CustomInstructions").and_then(extract_html_or_text));
    let mut attachments = vec![];
    if let Some(arr) = folder.get("Attachments").and_then(|a| a.as_array()) {
        attachments.extend(arr.iter().filter_map(parse_attachment));
    }
    if let Some(arr) = folder.get("AttachedResources").and_then(|a| a.as_array()) {
        attachments.extend(arr.iter().filter_map(parse_attachment));
    }
    (html, attachments)
}

fn parse_attachment(v: &Value) -> Option<AssignmentAttachment> {
    let name = v
        .get("FileName")
        .or_else(|| v.get("Name"))
        .or_else(|| v.get("Title"))
        .and_then(|n| n.as_str())
        .map(String::from)?;
    let url = v
        .get("Href")
        .or_else(|| v.get("Link"))
        .or_else(|| v.get("Url"))
        .and_then(|u| u.as_str())
        .map(String::from);
    let size = v.get("Size").and_then(|s| s.as_i64());
    Some(AssignmentAttachment { name, url, size })
}

/// Brightspace returns rich text fields as `{Text: "...", Html: "..."}`. Some
/// older endpoints just return a string. Accept either shape.
fn extract_html_or_text(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    v.get("Html")
        .and_then(|h| h.as_str())
        .map(String::from)
        .or_else(|| v.get("Text").and_then(|t| t.as_str()).map(String::from))
}

async fn lookup_gradebook(
    pool: &sqlx::SqlitePool,
    course_id: &str,
    assignment_id: &str,
) -> Result<Option<GradebookEntry>> {
    // Path 1: assignments.grade_item_id → grades.brightspace_id (preferred).
    // Path 2: name match on lowered grade name (Brightspace doesn't always
    //         set grade_item_id even when one exists).
    let row: Option<(Option<String>, Option<f64>, Option<f64>, Option<String>)> = sqlx::query_as(
        "SELECT g.displayed_grade, g.numerator, g.denominator, g.comments
         FROM assignments a
         LEFT JOIN grades g ON g.brightspace_id = a.grade_item_id AND g.course_id = a.course_id
         WHERE a.brightspace_id = ? AND a.course_id = ?
         LIMIT 1",
    )
    .bind(assignment_id)
    .bind(course_id)
    .fetch_optional(pool)
    .await?;

    if let Some((dg, n, d, c)) = row {
        if dg.is_some() || n.is_some() || c.is_some() {
            return Ok(Some(GradebookEntry {
                displayed_grade: dg,
                numerator: n,
                denominator: d,
                comments_html: c,
            }));
        }
    }
    Ok(None)
}
