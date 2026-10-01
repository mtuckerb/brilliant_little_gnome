// Course class-list sync. Brightspace's Classlist API already applies the
// caller's permissions and the institution's field-level privacy settings, so
// Brilliant persists only the visible display fields needed by Overview.

use crate::error::{AppError, Result};
use crate::state::AppState;
use serde_json::Value;
use sqlx::SqlitePool;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RosterSeed {
    brightspace_user_id: String,
    display_name: String,
    first_name: Option<String>,
    last_name: Option<String>,
    email: Option<String>,
    role_name: Option<String>,
    role_id: Option<String>,
    pronouns: Option<String>,
}

/// Refresh one course's saved class list. A zero-item response is treated as
/// inconclusive so a transient permission/configuration problem cannot erase
/// the last-known roster the user asked Brilliant to keep.
pub async fn sync(state: &AppState, course_id: &str) -> Result<usize> {
    let raw = state.client.get_classlist(course_id).await?;
    save_roster(&state.pool, course_id, &raw).await
}

async fn save_roster(pool: &SqlitePool, course_id: &str, raw: &[Value]) -> Result<usize> {
    let people = parse_classlist(raw);
    if people.is_empty() {
        return Err(AppError::Other(
            "Brightspace returned no visible class-list entries. Any saved list was kept.".into(),
        ));
    }

    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM course_roster WHERE course_id = ?")
        .bind(course_id)
        .execute(&mut *tx)
        .await?;

    for person in &people {
        sqlx::query(
            "INSERT INTO course_roster
               (course_id, brightspace_user_id, display_name, first_name, last_name,
                email, role_name, role_id, pronouns, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )
        .bind(course_id)
        .bind(&person.brightspace_user_id)
        .bind(&person.display_name)
        .bind(&person.first_name)
        .bind(&person.last_name)
        .bind(&person.email)
        .bind(&person.role_name)
        .bind(&person.role_id)
        .bind(&person.pronouns)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(people.len())
}

fn parse_classlist(items: &[Value]) -> Vec<RosterSeed> {
    let mut people: Vec<RosterSeed> = items
        .iter()
        .filter_map(parse_person)
        .filter(|person| {
            !person
                .role_name
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("preview")
        })
        .collect();

    people.sort_by_cached_key(|person| {
        format!(
            "{}\u{0}{}\u{0}{}",
            person
                .last_name
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase(),
            person
                .first_name
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase(),
            person.display_name.to_ascii_lowercase(),
        )
    });
    people
}

fn parse_person(item: &Value) -> Option<RosterSeed> {
    let brightspace_user_id = value_to_string(item.get("Identifier"))?;
    let first_name = clean_string(item.get("FirstName"));
    let last_name = clean_string(item.get("LastName"));
    let display_name = clean_string(item.get("DisplayName")).or_else(|| {
        let joined = [first_name.as_deref(), last_name.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        (!joined.is_empty()).then_some(joined)
    })?;

    Some(RosterSeed {
        brightspace_user_id,
        display_name,
        first_name,
        last_name,
        email: clean_string(item.get("Email")),
        role_name: clean_string(item.get("ClasslistRoleDisplayName")),
        role_id: value_to_string(item.get("RoleId")),
        pronouns: clean_string(item.get("Pronouns")),
    })
}

fn clean_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn value_to_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_classlist, save_roster};
    use serde_json::json;

    #[tokio::test]
    async fn roster_refresh_is_atomic_and_preserves_saved_rows_on_empty_or_failed_updates() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0020_course_roster.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let person = json!({"Identifier": "1", "DisplayName": "Saved student"});
        save_roster(&pool, "a", &[person.clone()]).await.unwrap();
        save_roster(&pool, "b", &[person.clone()]).await.unwrap();
        sqlx::query("UPDATE course_roster SET updated_at = '2026-01-01 00:00:00'")
            .execute(&pool)
            .await
            .unwrap();

        assert!(save_roster(&pool, "a", &[]).await.is_err());
        // A duplicate key fails after the DELETE and first INSERT: the old
        // snapshot must still survive the transaction rollback.
        assert!(save_roster(&pool, "a", &[person.clone(), person])
            .await
            .is_err());
        let saved: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT course_id, display_name, updated_at FROM course_roster ORDER BY course_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(saved.len(), 2);
        assert!(saved
            .iter()
            .all(|(_, name, timestamp)| name == "Saved student"
                && timestamp == "2026-01-01 00:00:00"));

        save_roster(
            &pool,
            "a",
            &[json!({"Identifier": "2", "DisplayName": "New student"})],
        )
        .await
        .unwrap();
        let saved: Vec<(String, String)> =
            sqlx::query_as("SELECT course_id, display_name FROM course_roster ORDER BY course_id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            saved,
            vec![
                ("a".into(), "New student".into()),
                ("b".into(), "Saved student".into())
            ]
        );
    }

    #[test]
    fn maps_visible_classlist_fields_and_sorts_by_name() {
        let people = parse_classlist(&[
            json!({
                "Identifier": "2",
                "DisplayName": "Ada Zephyr",
                "FirstName": "Ada",
                "LastName": "Zephyr",
                "Email": "ada@example.edu",
                "ClasslistRoleDisplayName": "Learner",
                "RoleId": 110,
                "Pronouns": "she/her"
            }),
            json!({
                "Identifier": "1",
                "DisplayName": "Morgan Alpha",
                "FirstName": "Morgan",
                "LastName": "Alpha",
                "Email": "",
                "ClasslistRoleDisplayName": "Instructor",
                "RoleId": null
            }),
        ]);

        assert_eq!(people.len(), 2);
        assert_eq!(people[0].display_name, "Morgan Alpha");
        assert_eq!(people[1].brightspace_user_id, "2");
        assert_eq!(people[1].role_id.as_deref(), Some("110"));
        assert_eq!(people[1].pronouns.as_deref(), Some("she/her"));
        assert_eq!(people[0].email, None);
    }

    #[test]
    fn skips_preview_accounts_and_unidentifiable_rows() {
        let people = parse_classlist(&[
            json!({
                "Identifier": "preview",
                "DisplayName": "Learner Preview",
                "ClasslistRoleDisplayName": "Preview Learner"
            }),
            json!({
                "DisplayName": "Missing identifier",
                "ClasslistRoleDisplayName": "Learner"
            }),
            json!({
                "Identifier": 7,
                "FirstName": "Fallback",
                "LastName": "Name",
                "ClasslistRoleDisplayName": "Learner"
            }),
        ]);

        assert_eq!(people.len(), 1);
        assert_eq!(people[0].brightspace_user_id, "7");
        assert_eq!(people[0].display_name, "Fallback Name");
    }
}
