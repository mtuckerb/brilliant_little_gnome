// Local-first course class list commands. Overview reads the saved snapshot;
// the first read after this feature is installed bootstraps from Brightspace,
// while an explicit refresh always bypasses the short HTTP cache.

use super::AppStateArg;
use crate::commands::downloads::{emit_saved, save_download_file, DownloadBytes};
use crate::error::{AppError, Result};
use serde::Serialize;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CourseRosterPerson {
    pub brightspace_user_id: String,
    pub display_name: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub role_name: Option<String>,
    pub pronouns: Option<String>,
    #[sqlx(skip)]
    pub is_current_user: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CourseRoster {
    pub people: Vec<CourseRosterPerson>,
    pub synced_at: Option<String>,
}

#[tauri::command]
pub async fn get_course_roster(state: AppStateArg<'_>, course_id: String) -> Result<CourseRoster> {
    let saved = load_roster(state.inner().as_ref(), &course_id).await?;
    if !saved.people.is_empty() || !state.client.is_configured() {
        return Ok(saved);
    }

    crate::sync::roster::sync(state.inner().as_ref(), &course_id).await?;
    load_roster(state.inner().as_ref(), &course_id).await
}

#[tauri::command]
pub async fn refresh_course_roster(
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<CourseRoster> {
    crate::sync::roster::sync(state.inner().as_ref(), &course_id).await?;
    load_roster(state.inner().as_ref(), &course_id).await
}

#[tauri::command]
pub async fn download_course_roster(
    app: AppHandle,
    state: AppStateArg<'_>,
    course_id: String,
) -> Result<DownloadBytes> {
    let mut roster = load_roster(state.inner().as_ref(), &course_id).await?;
    if roster.people.is_empty() {
        crate::sync::roster::sync(state.inner().as_ref(), &course_id).await?;
        roster = load_roster(state.inner().as_ref(), &course_id).await?;
    }
    if roster.people.is_empty() {
        return Err(AppError::Other(
            "No class-list entries are available to download.".to_string(),
        ));
    }

    let mut csv = String::from("\u{feff}Name,Email,Role,Pronouns\r\n");
    for person in &roster.people {
        let fields = [
            csv_cell(&person.display_name),
            csv_cell(person.email.as_deref().unwrap_or_default()),
            csv_cell(person.role_name.as_deref().unwrap_or_default()),
            csv_cell(person.pronouns.as_deref().unwrap_or_default()),
        ];
        csv.push_str(&fields.join(","));
        csv.push_str("\r\n");
    }

    let filename = format!("Brilliant-{}-Class-List.csv", course_id);
    let saved_path = save_download_file(&state.app, &filename, csv.as_bytes())?;
    let payload = DownloadBytes {
        bytes_base64: None,
        mime: Some("text/csv;charset=utf-8".to_string()),
        filename,
        saved_path: Some(saved_path.display().to_string()),
    };
    emit_saved(&app, &payload);
    Ok(payload)
}

async fn load_roster(state: &crate::state::AppState, course_id: &str) -> Result<CourseRoster> {
    let mut people = sqlx::query_as::<_, CourseRosterPerson>(
        "SELECT brightspace_user_id, display_name, first_name, last_name, email,
                role_name, pronouns
         FROM course_roster
         WHERE course_id = ?
         ORDER BY
           CASE WHEN LOWER(COALESCE(role_name, '')) LIKE '%instructor%' THEN 0 ELSE 1 END,
           LOWER(COALESCE(last_name, display_name)),
           LOWER(COALESCE(first_name, display_name))",
    )
    .bind(course_id)
    .fetch_all(&state.pool)
    .await?;

    let current_user_id = state.client.user_id_clone();
    for person in &mut people {
        person.is_current_user = current_user_id
            .as_deref()
            .map(|id| id == person.brightspace_user_id)
            .unwrap_or(false);
    }

    let synced_at = sqlx::query_scalar::<_, Option<String>>(
        "SELECT MAX(updated_at) FROM course_roster WHERE course_id = ?",
    )
    .bind(course_id)
    .fetch_one(&state.pool)
    .await?;

    Ok(CourseRoster { people, synced_at })
}

/// Quote every CSV field and neutralize formula-like prefixes so opening a
/// roster in a spreadsheet cannot interpret a name or address as a formula.
fn csv_cell(value: &str) -> String {
    let flattened = value.replace(|c| matches!(c, '\r' | '\n'), " ");
    let formula_like = flattened
        .chars()
        .next()
        .map(|c| matches!(c, '=' | '+' | '-' | '@'))
        .unwrap_or(false);
    let safe = if formula_like {
        format!("'{}", flattened)
    } else {
        flattened
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::csv_cell;

    #[test]
    fn csv_cell_quotes_and_neutralizes_formula_prefixes() {
        assert_eq!(csv_cell("Last, First"), "\"Last, First\"");
        assert_eq!(
            csv_cell("=HYPERLINK(\"bad\")"),
            "\"'=HYPERLINK(\"\"bad\"\")\""
        );
        assert_eq!(csv_cell("line one\nline two"), "\"line one line two\"");
    }
}
