use super::AppStateArg;
use crate::error::{AppError, Result};
use base64::Engine;
use serde::Serialize;
use sqlx::SqlitePool;

pub const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;
static IMPORT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Student {
    pub brightspace_user_id: String,
    pub display_name: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub pronouns: Option<String>,
    pub avatar: Option<String>,
    pub avatar_overridden: bool,
    #[sqlx(skip)]
    pub courses: Vec<StudentCourse>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StudentCourse {
    pub course_id: String,
    pub name: String,
    pub semester: Option<String>,
    pub available: bool,
}

#[derive(Serialize)]
pub struct StudentImportResult {
    pub imported_courses: usize,
    pub failed_courses: Vec<String>,
}

#[tauri::command]
pub async fn list_students(state: AppStateArg<'_>) -> Result<Vec<Student>> {
    let current_user = state.client.user_id_clone();
    load_students(&state.pool, current_user.as_deref()).await
}

pub(crate) async fn load_students(
    pool: &SqlitePool,
    current_user: Option<&str>,
) -> Result<Vec<Student>> {
    let mut students = sqlx::query_as::<_, Student>(
        "SELECT brightspace_user_id, display_name, first_name, last_name, email, pronouns,
            CASE WHEN avatar_overridden THEN custom_avatar ELSE imported_avatar END AS avatar,
            avatar_overridden
         FROM students WHERE brightspace_user_id != COALESCE(?,
           (SELECT brightspace_user_id FROM user_preferences LIMIT 1), '')
         ORDER BY LOWER(COALESCE(last_name, display_name)), LOWER(COALESCE(first_name, display_name)), brightspace_user_id",
    ).bind(current_user).fetch_all(pool).await?;
    // One query for all memberships, rather than one per student.
    let memberships = sqlx::query_as::<_, (String, String, String, Option<String>, bool)>(
        "SELECT sc.brightspace_user_id, sc.course_id, COALESCE(c.name, sc.course_name),
            COALESCE(c.semester, sc.semester), c.org_unit_id IS NOT NULL
         FROM student_courses sc LEFT JOIN courses c ON c.org_unit_id = sc.course_id
         ORDER BY COALESCE(c.semester, sc.semester) DESC, LOWER(COALESCE(c.name, sc.course_name))",
    )
    .fetch_all(pool)
    .await?;
    let mut by_student = std::collections::HashMap::<String, Vec<StudentCourse>>::new();
    for (id, course_id, name, semester, available) in memberships {
        by_student.entry(id).or_default().push(StudentCourse {
            course_id,
            name,
            semester,
            available,
        });
    }
    for student in &mut students {
        student.courses = by_student
            .remove(&student.brightspace_user_id)
            .unwrap_or_default();
    }
    Ok(students)
}

/// Bootstrap every locally known course, including archived/dropped courses.
/// Failed/empty rosters remain eligible for retry and never remove history.
#[tauri::command]
pub async fn import_students(state: AppStateArg<'_>, force: bool) -> Result<StudentImportResult> {
    // React remounts and multiple windows can request initial import together.
    // Recheck import markers after the preceding request has finished.
    let _guard = IMPORT_LOCK.lock().await;
    if !state.client.is_configured() {
        return Ok(StudentImportResult {
            imported_courses: 0,
            failed_courses: vec![],
        });
    }
    let courses = sqlx::query_as::<_, (String, String)>(
        "SELECT org_unit_id, name FROM courses WHERE ? OR org_unit_id NOT IN
           (SELECT course_id FROM student_roster_imports) ORDER BY org_unit_id",
    )
    .bind(force)
    .fetch_all(&state.pool)
    .await?;
    let mut result = StudentImportResult {
        imported_courses: 0,
        failed_courses: vec![],
    };
    for (id, name) in courses {
        match crate::sync::roster::sync(state.inner().as_ref(), &id, force).await {
            Ok(count) if count > 0 => {
                result.imported_courses += 1;
                state.events.course_updated(&id);
            }
            _ => result.failed_courses.push(name),
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn update_student_avatar(
    state: AppStateArg<'_>,
    student_id: String,
    avatar: Option<String>,
) -> Result<()> {
    if let Some(data) = &avatar {
        validate_avatar(data)?;
    }
    save_avatar(&state.pool, &student_id, avatar.as_deref()).await?;
    state.events.emit("students:updated", ());
    Ok(())
}

async fn save_avatar(pool: &SqlitePool, id: &str, avatar: Option<&str>) -> Result<()> {
    let result = sqlx::query(
        "UPDATE students SET custom_avatar = ?, avatar_overridden = 1, updated_at = CURRENT_TIMESTAMP
         WHERE brightspace_user_id = ?",
    ).bind(avatar).bind(id).execute(pool).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::Other("Student not found.".into()));
    }
    Ok(())
}

/// Persist only bounded raster image data; no URLs that can expire or leak requests.
pub(crate) fn validate_avatar(data: &str) -> Result<()> {
    if data.len() > MAX_AVATAR_BYTES * 4 / 3 + 128 {
        return Err(AppError::Other("Choose an image smaller than 2 MB.".into()));
    }
    let (header, encoded) = data
        .split_once(',')
        .ok_or_else(|| AppError::Other("Invalid avatar image.".into()))?;
    if !matches!(
        header,
        "data:image/png;base64"
            | "data:image/jpeg;base64"
            | "data:image/webp;base64"
            | "data:image/gif;base64"
    ) {
        return Err(AppError::Other(
            "Choose a PNG, JPEG, WebP, or GIF image.".into(),
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| AppError::Other("Invalid avatar image.".into()))?;
    if bytes.is_empty() || bytes.len() > MAX_AVATAR_BYTES {
        return Err(AppError::Other("Choose an image smaller than 2 MB.".into()));
    }
    let format =
        image::guess_format(&bytes).map_err(|_| AppError::Other("Invalid avatar image.".into()))?;
    let expected = match format {
        image::ImageFormat::Png => "data:image/png;base64",
        image::ImageFormat::Jpeg => "data:image/jpeg;base64",
        image::ImageFormat::WebP => "data:image/webp;base64",
        image::ImageFormat::Gif => "data:image/gif;base64",
        _ => return Err(AppError::Other("Unsupported avatar image.".into())),
    };
    if header != expected {
        return Err(AppError::Other("Invalid avatar image type.".into()));
    }
    Ok(())
}

pub(crate) fn avatar_data_url(bytes: &[u8]) -> Option<String> {
    let mime = match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        image::ImageFormat::Gif => "image/gif",
        _ => return None,
    };
    let data = format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    validate_avatar(&data).ok()?;
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn students_backfill_deduplicates_and_preserves_history_and_custom_avatars() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("students.sqlite");
        let connect = || {
            sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    sqlx::sqlite::SqliteConnectOptions::new()
                        .filename(&path)
                        .create_if_missing(true),
                )
        };
        let pool = connect().await.unwrap();
        // Simulate upgrading an installation with current and archived class lists.
        for migration in [
            include_str!("../../migrations/0001_init.sql"),
            include_str!("../../migrations/0020_course_roster.sql"),
        ] {
            sqlx::raw_sql(migration).execute(&pool).await.unwrap();
        }
        sqlx::raw_sql(
            "INSERT INTO user_preferences (brightspace_user_id) VALUES ('me');
             INSERT INTO courses (org_unit_id, name, status) VALUES ('old', 'Old class', 'archived'), ('new', 'New class', 'active');
             INSERT INTO course_roster (course_id, brightspace_user_id, display_name, role_name) VALUES
               ('old', 'ada', 'Ada', 'Learner'), ('new', 'ada', 'Ada', 'Student'),
               ('new', 'me', 'Me', 'Student'), ('old', 'teacher', 'Teacher', 'Instructor'),
               ('new', 'preview', 'Preview', 'Preview Learner');",
        ).execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0022_students.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let students = load_students(&pool, None).await.unwrap();
        assert_eq!(students.len(), 1);
        assert_eq!(students[0].courses.len(), 2);
        assert_eq!(students[0].brightspace_user_id, "ada");
        assert!(students[0].courses.iter().any(|c| c.course_id == "old"));

        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aWQAAAABJRU5ErkJggg==";
        validate_avatar(png).unwrap();
        save_avatar(&pool, "ada", Some(png)).await.unwrap();
        sqlx::query(
            "UPDATE students SET imported_avatar = 'new import' WHERE brightspace_user_id = 'ada'",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Roster refresh: Ada disappears; a new learner appears.
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("DELETE FROM course_roster WHERE course_id = 'new'")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO course_roster (course_id, brightspace_user_id, display_name, role_name) VALUES ('new', 'grace', 'Grace', 'Learner')")
            .execute(&mut *tx).await.unwrap();
        crate::sync::roster::archive_students(&mut tx, "new")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        pool.close().await;
        // A real file close/reopen, rather than merely reading the same connection.
        let pool = connect().await.unwrap();
        let students = load_students(&pool, None).await.unwrap();
        assert_eq!(students.len(), 2);
        let ada = students
            .iter()
            .find(|s| s.brightspace_user_id == "ada")
            .unwrap();
        assert_eq!(ada.avatar.as_deref(), Some(png));
        assert!(ada.avatar_overridden);
        assert_eq!(ada.courses.len(), 2);
        // Removal is itself a durable override: the imported image stays hidden.
        save_avatar(&pool, "ada", None).await.unwrap();
        sqlx::query("DELETE FROM courses WHERE org_unit_id = 'old'")
            .execute(&pool)
            .await
            .unwrap();
        let students = load_students(&pool, None).await.unwrap();
        let ada = students
            .iter()
            .find(|s| s.brightspace_user_id == "ada")
            .unwrap();
        assert!(ada.avatar.is_none());
        assert!(ada.avatar_overridden);
        assert_eq!(ada.courses.len(), 2);
        let old = ada.courses.iter().find(|c| c.course_id == "old").unwrap();
        assert_eq!(old.name, "Old class");
        assert!(!old.available);
    }

    #[tokio::test]
    async fn roster_reimports_update_names_without_touching_avatar_overrides() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO course_roster (course_id, brightspace_user_id, display_name, role_name, email) VALUES ('course', '1', 'Original', 'Student', 'first@example.edu')")
            .execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        crate::sync::roster::archive_students(&mut tx, "course")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        save_avatar(&pool, "1", Some("custom photo")).await.unwrap();
        sqlx::query("UPDATE course_roster SET display_name = 'Updated', email = NULL")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        crate::sync::roster::archive_students(&mut tx, "course")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let students = load_students(&pool, None).await.unwrap();
        assert_eq!(students.len(), 1);
        assert_eq!(students[0].display_name, "Updated");
        assert_eq!(students[0].email.as_deref(), Some("first@example.edu"));
        assert_eq!(students[0].avatar.as_deref(), Some("custom photo"));
        assert_eq!(students[0].courses.len(), 1);
    }

    #[test]
    fn avatars_reject_remote_urls_svg_invalid_data_and_oversized_payloads() {
        for value in [
            "https://example.edu/expiring-photo",
            "data:image/svg+xml;base64,PHN2Zz4=",
            "data:image/png;base64,bm90IGFuIGltYWdl",
            "data:image/png;base64,%%",
        ] {
            assert!(validate_avatar(value).is_err(), "accepted {value}");
        }
        assert!(validate_avatar(&"x".repeat(MAX_AVATAR_BYTES * 2)).is_err());
    }
}
