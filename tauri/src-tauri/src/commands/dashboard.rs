use super::grades::calculate_summary;
use super::AppStateArg;
use crate::error::Result;
use crate::models::Grade;
use chrono::Datelike;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Serialize)]
pub struct DashboardGpa {
    pub semester: String,
    pub locked_in_gpa: Option<f64>,
    pub projected_gpa: Option<f64>,
    /// Credit-weighted official grade coverage, from 0 to 1; estimates never increase it.
    pub confidence: f64,
    pub locked_in_units: f64,
    pub projected_units: f64,
    pub provisional_course_count: usize,
    pub projected_course_count: usize,
    pub excluded_course_count: usize,
}

#[derive(Debug, sqlx::FromRow)]
struct GpaCourse {
    org_unit_id: String,
    semester: Option<String>,
    units: Option<f64>,
    target_grade: Option<f64>,
    status: Option<String>,
}

#[tauri::command]
pub async fn dashboard_gpa(state: AppStateArg<'_>) -> Result<DashboardGpa> {
    dashboard_gpa_in(&state.pool).await
}

pub async fn dashboard_gpa_in(pool: &SqlitePool) -> Result<DashboardGpa> {
    let now = chrono::Local::now();
    dashboard_gpa_at(pool, calendar_semester(now.year(), now.month())).await
}

async fn dashboard_gpa_at(pool: &SqlitePool, calendar_term: (i32, i32)) -> Result<DashboardGpa> {
    // One SQLite snapshot and three batch reads, independent of the number of courses.
    let mut tx = pool.begin().await?;
    let prefs: Option<(Option<f64>, Option<f64>, Option<String>)> = sqlx::query_as(
        "SELECT historic_gpa, historic_units, default_semester FROM user_preferences LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let courses = sqlx::query_as::<_, GpaCourse>(
        "SELECT org_unit_id, COALESCE(NULLIF(custom_semester, ''), semester) AS semester, units, target_grade, status FROM courses",
    )
    .fetch_all(&mut *tx)
    .await?;
    let grades = sqlx::query_as::<_, Grade>(
        "SELECT id, course_id, brightspace_id, name, displayed_grade, numerator, denominator, weight, due_date, is_extra_credit, hidden, manually_marked_ungraded, expected_score, comments FROM grades WHERE hidden = 0",
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let (historic_gpa, historic_units, default_semester) = prefs.unwrap_or_default();
    Ok(calculate_gpa(
        &courses,
        grades,
        historic_gpa,
        historic_units,
        default_semester.as_deref(),
        calendar_term,
    ))
}

// Same 4-point scale used by the legacy cumulative GPA calculation.
fn to_gpa(score: f64) -> f64 {
    for (minimum, gpa) in [
        (93.0, 4.0),
        (90.0, 3.67),
        (87.0, 3.33),
        (83.0, 3.0),
        (80.0, 2.67),
        (77.0, 2.33),
        (73.0, 2.0),
        (70.0, 1.67),
        (67.0, 1.33),
        (63.0, 1.0),
        (60.0, 0.67),
    ] {
        if score >= minimum {
            return gpa;
        }
    }
    0.0
}

/// Match both "Fall 2026" and "2026 Fall", plus Brightspace's short term labels.
fn semester_key(semester: &str) -> Option<(i32, i32)> {
    let lower = semester.trim().to_lowercase();
    static YEAR_RE: OnceLock<regex::Regex> = OnceLock::new();
    let year_re = YEAR_RE.get_or_init(|| regex::Regex::new(r"20\d{2}").expect("static year regex"));
    let year_match = year_re.find(&lower)?;
    let year = year_match.as_str().parse().ok()?;
    let suffix = lower[year_match.end()..].trim();
    let season = if lower.contains("winter") || suffix == "wi" {
        0
    } else if lower.contains("spring") || suffix == "sp" {
        1
    } else if lower.contains("summer") || lower.contains("session") || suffix == "su" {
        2
    } else if lower.contains("fall") || lower.contains("autumn") || suffix == "fa" {
        3
    } else {
        return None;
    };
    Some((year, season))
}

// Conservative calendar bounds: Spring through May, Summer through August, Fall
// through December. A configured Winter term can keep that short session current.
fn calendar_semester(year: i32, month: u32) -> (i32, i32) {
    (
        year,
        match month {
            1..=5 => 1,
            6..=8 => 2,
            _ => 3,
        },
    )
}

fn calculate_gpa(
    courses: &[GpaCourse],
    grades: Vec<Grade>,
    historic_gpa: Option<f64>,
    historic_units: Option<f64>,
    default_semester: Option<&str>,
    calendar_term: (i32, i32),
) -> DashboardGpa {
    let mut by_course: HashMap<String, Vec<Grade>> = HashMap::new();
    for grade in grades {
        by_course
            .entry(grade.course_id.clone())
            .or_default()
            .push(grade);
    }
    // Enrollment is not evidence of completion. In particular, an empty future
    // shell must never turn current grades into a completed, 100%-certain result.
    // Preserve an explicit earlier overview term, while bounding future settings.
    let current_semester = default_semester
        .and_then(semester_key)
        .map(|term| term.min(calendar_term))
        .unwrap_or(calendar_term);
    let mut locked_points = 0.0;
    let mut locked_units = 0.0;
    if let (Some(gpa), Some(units)) = (historic_gpa, historic_units) {
        if gpa.is_finite() && (0.0..=4.0).contains(&gpa) && units.is_finite() && units > 0.0 {
            locked_points = gpa * units;
            locked_units = units;
        }
    }
    let mut projected_points = locked_points;
    let mut projected_units = locked_units;
    let mut official_units = locked_units;
    let mut provisional_course_count = 0;
    let mut projected_course_count = 0;
    let mut excluded_course_count = 0;

    for course in courses {
        if matches!(
            course.status.as_deref(),
            Some("withdrawn" | "early_withdrawal" | "dropped")
        ) {
            continue;
        }
        let units = course.units.unwrap_or(3.0);
        if !units.is_finite() || units <= 0.0 {
            continue;
        }
        let term = course.semester.as_deref().and_then(semester_key);
        if term.is_some_and(|term| term > current_semester) {
            continue;
        }
        if course.status.as_deref() == Some("dropped_fail") {
            // A withdrawal with failure is an official zero, never a missing grade.
            locked_units += units;
            projected_units += units;
            official_units += units;
            continue;
        }
        let course_grades = by_course.remove(&course.org_unit_id).unwrap_or_default();
        let target = course
            .target_grade
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(93.0);
        let summary = calculate_summary(&course_grades, target);
        let has_provisional = summary.rows.iter().any(|row| row.is_expected);
        let official_possible: f64 = summary
            .rows
            .iter()
            .filter(|row| row.is_graded)
            .map(|row| row.grade.denominator.unwrap_or(0.0))
            .sum();
        let past_term = term.is_some_and(|term| term < current_semester);
        // A current gradebook can be fully marked before the instructor adds future work.
        // Only completed terms or explicitly completed courses enter the locked-in record.
        let completed =
            !has_provisional && (past_term || course.status.as_deref() == Some("completed"));
        let score = summary.stats.score.filter(|score| score.is_finite());
        if completed {
            if let Some(score) = score {
                let points = to_gpa(score) * units;
                locked_points += points;
                locked_units += units;
                projected_points += points;
                projected_units += units;
                official_units += units;
            } else {
                excluded_course_count += 1;
            }
            continue;
        }
        // Do not turn an undated, empty shell or unavailable historic record into an A.
        if score.is_none() && (term.is_none() || past_term) {
            excluded_course_count += 1;
            continue;
        }
        let all_possible = summary.stats.all_possible_points;
        let projected_score = if all_possible > 0.0 {
            (summary.stats.total_points_earned + summary.stats.remaining_points * target / 100.0)
                / all_possible
                * 100.0
        } else {
            target
        };
        projected_points += to_gpa(projected_score) * units;
        projected_units += units;
        official_units += if all_possible > 0.0 {
            (official_possible / all_possible).clamp(0.0, 1.0) * units
        } else {
            0.0
        };
        provisional_course_count += usize::from(has_provisional);
        projected_course_count += 1;
    }
    let (year, season) = current_semester;
    DashboardGpa {
        semester: format!(
            "{} {}",
            year,
            ["Winter", "Spring", "Summer", "Fall"][season as usize]
        ),
        locked_in_gpa: (locked_units > 0.0).then(|| locked_points / locked_units),
        projected_gpa: (projected_units > 0.0).then(|| projected_points / projected_units),
        confidence: if projected_units > 0.0 {
            (official_units / projected_units).clamp(0.0, 1.0)
        } else {
            0.0
        },
        locked_in_units: locked_units,
        projected_units,
        provisional_course_count,
        projected_course_count,
        excluded_course_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn course(id: &str, semester: &str, units: f64) -> GpaCourse {
        GpaCourse {
            org_unit_id: id.into(),
            semester: Some(semester.into()),
            units: Some(units),
            target_grade: Some(90.0),
            status: Some("active".into()),
        }
    }
    fn grade(id: &str, numerator: Option<f64>, denominator: f64, expected: Option<f64>) -> Grade {
        Grade {
            id: 1,
            course_id: id.into(),
            brightspace_id: Some("grade".into()),
            name: "Quiz".into(),
            displayed_grade: None,
            numerator,
            denominator: Some(denominator),
            weight: None,
            due_date: None,
            is_extra_credit: false,
            hidden: false,
            manually_marked_ungraded: false,
            expected_score: expected,
            comments: None,
        }
    }
    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.00001,
            "{actual} != {expected}"
        );
    }
    fn summary(courses: &[GpaCourse], grades: Vec<Grade>) -> DashboardGpa {
        calculate_gpa(courses, grades, None, None, Some("2026 Fall"), (2026, 3))
    }

    #[test]
    fn weights_completed_and_projected_courses_by_credits() {
        let result = summary(
            &[
                course("old", "Fall 2025", 3.0),
                course("current", "2026FA", 6.0),
            ],
            vec![
                grade("old", Some(95.0), 100.0, None),
                grade("current", Some(40.0), 50.0, None),
                grade("current", None, 50.0, None),
            ],
        );
        near(result.locked_in_gpa.unwrap(), 4.0);
        // Current course: (40 earned + 50 remaining * 90% target) / 100 = 85%, or 3.0.
        near(result.projected_gpa.unwrap(), (4.0 * 3.0 + 3.0 * 6.0) / 9.0);
        near(result.confidence, (3.0 + 6.0 * 0.5) / 9.0);
        assert_eq!(result.locked_in_units, 3.0);
        assert_eq!(result.projected_units, 9.0);
    }

    #[test]
    fn historic_gpa_is_credit_weighted_and_an_official_zero_is_valid() {
        let result = calculate_gpa(
            &[course("old", "2025 Fall", 3.0)],
            vec![grade("old", Some(0.0), 100.0, None)],
            Some(3.5),
            Some(60.0),
            Some("2026 Fall"),
            (2026, 3),
        );
        near(result.locked_in_gpa.unwrap(), 3.5 * 60.0 / 63.0);
        near(result.projected_gpa.unwrap(), result.locked_in_gpa.unwrap());
        assert_eq!(result.confidence, 1.0);
        let historic_only = calculate_gpa(&[], vec![], Some(0.0), Some(6.0), None, (2026, 3));
        assert_eq!(historic_only.locked_in_gpa, Some(0.0));
        assert_eq!(historic_only.confidence, 1.0);
    }

    #[test]
    fn provisional_scores_change_projection_without_increasing_confidence() {
        let courses = [course("current", "2026 Fall", 3.0)];
        let without = summary(
            &courses,
            vec![
                grade("current", Some(40.0), 50.0, None),
                grade("current", None, 50.0, None),
            ],
        );
        let provisional = summary(
            &courses,
            vec![
                grade("current", Some(40.0), 50.0, None),
                grade("current", None, 50.0, Some(100.0)),
            ],
        );
        let official = summary(
            &courses,
            vec![
                grade("current", Some(40.0), 50.0, None),
                grade("current", Some(0.0), 50.0, Some(100.0)),
            ],
        );
        assert_eq!(without.projected_gpa, Some(3.0));
        assert_eq!(provisional.projected_gpa, Some(3.67));
        assert_eq!(without.confidence, provisional.confidence);
        assert_eq!(provisional.confidence, 0.5);
        assert_eq!(provisional.provisional_course_count, 1);
        assert_eq!(official.projected_gpa, Some(0.0));
        assert_eq!(official.confidence, 1.0);
        assert_eq!(official.provisional_course_count, 0);
        assert!(official.locked_in_gpa.is_none());
    }

    #[test]
    fn current_gradebooks_and_past_provisional_scores_are_not_locked() {
        let current = summary(
            &[course("current", "2026 Fall", 3.0)],
            vec![grade("current", Some(95.0), 100.0, None)],
        );
        assert_eq!(current.locked_in_gpa, None);
        assert_eq!(current.projected_gpa, Some(4.0));
        let past = summary(
            &[course("old", "2025 Fall", 3.0)],
            vec![grade("old", None, 100.0, Some(95.0))],
        );
        assert_eq!(past.locked_in_gpa, None);
        assert_eq!(past.projected_gpa, Some(4.0));
        assert_eq!(past.confidence, 0.0);
    }

    #[test]
    fn withdrawals_and_future_or_zero_credit_courses_do_not_inflate_gpa() {
        let mut courses = vec![
            course("old", "2025 Fall", 3.0),
            course("failure", "2026 Fall", 3.0),
            course("future", "2027 Spring", 30.0),
            course("zero", "2026 Fall", 0.0),
        ];
        courses[1].status = Some("dropped_fail".into());
        for status in ["withdrawn", "early_withdrawal", "dropped"] {
            let mut withdrawn = course(status, "2025 Fall", 30.0);
            withdrawn.status = Some(status.into());
            courses.push(withdrawn);
        }
        let result = summary(&courses, vec![grade("old", Some(95.0), 100.0, None)]);
        assert_eq!(result.locked_in_gpa, Some(2.0));
        assert_eq!(result.projected_gpa, Some(2.0));
        assert_eq!(result.projected_units, 6.0);
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn missing_grades_are_unavailable_and_unstarted_current_courses_use_targets() {
        let mut undated = course("undated", "", 3.0);
        undated.semester = None;
        let missing = summary(&[course("old", "2025 Fall", 3.0), undated], vec![]);
        assert_eq!(missing.locked_in_gpa, None);
        assert_eq!(missing.projected_gpa, None);
        assert_eq!(missing.excluded_course_count, 2);
        let unstarted = summary(&[course("current", "2026 Fall", 3.0)], vec![]);
        assert_eq!(unstarted.projected_gpa, Some(3.67));
        assert_eq!(unstarted.confidence, 0.0);
    }

    #[test]
    fn hidden_and_manually_ungraded_rows_do_not_increase_confidence() {
        let mut hidden = grade("current", Some(100.0), 100.0, None);
        hidden.hidden = true;
        let mut ungraded = grade("current", Some(0.0), 100.0, Some(95.0));
        ungraded.manually_marked_ungraded = true;
        let result = summary(
            &[course("current", "2026 Fall", 3.0)],
            vec![hidden, ungraded],
        );
        assert_eq!(result.projected_gpa, Some(4.0));
        assert_eq!(result.confidence, 0.0);
        assert_eq!(result.locked_in_gpa, None);
    }

    #[test]
    fn defaults_to_three_credits_and_rejects_incomplete_historic_record() {
        let mut old = course("old", "2025 Fall", 3.0);
        old.units = None;
        let result = calculate_gpa(
            &[old],
            vec![grade("old", Some(95.0), 100.0, None)],
            None,
            Some(90.0),
            Some("2026 Fall"),
            (2026, 3),
        );
        assert_eq!(result.locked_in_units, 3.0);
        assert_eq!(result.locked_in_gpa, Some(4.0));
        let invalid = calculate_gpa(&[], vec![], Some(f64::NAN), Some(60.0), None, (2026, 3));
        assert!(invalid.projected_gpa.is_none());
    }

    #[test]
    fn normalizes_semesters_and_uses_calendar_when_default_is_absent() {
        for term in ["Fall 2026", "2026 Fall", "2026FA", "Autumn 2026"] {
            assert_eq!(semester_key(term), Some((2026, 3)));
        }
        assert_eq!(semester_key("2026SP"), Some((2026, 1)));
        assert_eq!(semester_key("Orientation"), None);
        let result = calculate_gpa(
            &[
                course("old", "2025 Fall", 3.0),
                course("current", "2026FA", 3.0),
            ],
            vec![grade("old", Some(95.0), 100.0, None)],
            None,
            None,
            None,
            (2026, 3),
        );
        assert_eq!(result.locked_in_units, 3.0);
        assert_eq!(result.projected_units, 6.0);
    }

    #[test]
    fn retains_established_gpa_boundaries() {
        for (score, expected) in [
            (100.0, 4.0),
            (93.0, 4.0),
            (92.99, 3.67),
            (90.0, 3.67),
            (87.0, 3.33),
            (83.0, 3.0),
            (80.0, 2.67),
            (77.0, 2.33),
            (73.0, 2.0),
            (70.0, 1.67),
            (67.0, 1.33),
            (63.0, 1.0),
            (60.0, 0.67),
            (59.99, 0.0),
            (0.0, 0.0),
        ] {
            assert_eq!(to_gpa(score), expected);
        }
    }

    #[tokio::test]
    async fn reads_saved_preferences_custom_semesters_and_provisional_grades_without_writes() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_preferences (historic_gpa, historic_units, default_semester) VALUES (3.5, 3, '2026 Fall')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO courses (org_unit_id, name, semester, custom_semester, units) VALUES ('old', 'Old', '2027 Spring', 'Fall 2025', 3), ('current', 'Current', '2026 Fall', NULL, 6)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO grades (course_id, brightspace_id, name, numerator, denominator, expected_score) VALUES ('old', 'old-grade', 'Quiz', 90, 100, NULL), ('current', 'new-grade', 'Quiz', NULL, 100, 100)").execute(&pool).await.unwrap();
        let result = dashboard_gpa_at(&pool, (2026, 3)).await.unwrap();
        near(result.locked_in_gpa.unwrap(), (3.5 + 3.67) / 2.0);
        near(
            result.projected_gpa.unwrap(),
            (3.5 * 3.0 + 3.67 * 3.0 + 4.0 * 6.0) / 12.0,
        );
        assert_eq!(result.confidence, 0.5);
        let saved: (Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT numerator, expected_score FROM grades WHERE course_id = 'current'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(saved, (None, Some(100.0)));
    }
    #[test]
    fn future_shells_cannot_lock_partial_current_grades_or_enter_projection() {
        for future_status in ["active", "withdrawn"] {
            for future_units in [0.0, 3.0] {
                let mut future = course("future", "2027 Spring", future_units);
                future.status = Some(future_status.into());
                let result = calculate_gpa(
                    &[course("current", "2026 Fall", 3.0), future],
                    vec![
                        grade("current", Some(40.0), 50.0, None),
                        grade("current", None, 50.0, None),
                    ],
                    None,
                    None,
                    None,
                    (2026, 3),
                );
                assert_eq!(result.locked_in_gpa, None);
                assert_eq!(result.projected_gpa, Some(3.0));
                assert_eq!(result.confidence, 0.5);
                assert_eq!(result.projected_units, 3.0);
                assert_eq!(result.semester, "2026 Fall");
            }
        }
    }

    #[test]
    fn history_only_latest_term_is_completed_and_future_override_is_bounded() {
        let past = calculate_gpa(
            &[course("past", "2026 Summer", 3.0)],
            vec![grade("past", Some(95.0), 100.0, None)],
            None,
            None,
            None,
            (2026, 3),
        );
        assert_eq!(past.locked_in_gpa, Some(4.0));
        assert_eq!(past.confidence, 1.0);
        let bounded = calculate_gpa(
            &[course("current", "2026 Fall", 3.0)],
            vec![
                grade("current", Some(40.0), 50.0, None),
                grade("current", None, 50.0, None),
            ],
            None,
            None,
            Some("2027 Spring"),
            (2026, 3),
        );
        assert_eq!(bounded.locked_in_gpa, None);
        assert_eq!(bounded.confidence, 0.5);
        assert_eq!(bounded.semester, "2026 Fall");
        let earlier = calculate_gpa(&[], vec![], None, None, Some("2026 Spring"), (2026, 3));
        assert_eq!(earlier.semester, "2026 Spring");
    }

    #[test]
    fn calendar_bounds_are_deterministic_at_term_and_year_changes() {
        for month in 1..=5 {
            assert_eq!(calendar_semester(2026, month), (2026, 1));
        }
        for month in 6..=8 {
            assert_eq!(calendar_semester(2026, month), (2026, 2));
        }
        for month in 9..=12 {
            assert_eq!(calendar_semester(2026, month), (2026, 3));
        }
        assert_eq!(calendar_semester(2027, 1), (2027, 1));
    }
}
