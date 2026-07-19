// Domain types. Mirrors src/types.ts on the frontend — keep them in sync.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Course {
    pub org_unit_id: String,
    pub name: String,
    pub custom_name: Option<String>,
    pub code: Option<String>,
    pub custom_code: Option<String>,
    pub semester: Option<String>,
    pub custom_semester: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub is_pinned: bool,
    pub custom_color: Option<String>,
    pub banner_url: Option<String>,
    pub units: Option<f64>,
    pub target_grade: Option<f64>,
    pub status: Option<String>,
    pub sort_order: Option<i64>,
    pub end_of_week_day: Option<i64>,
    pub last_accessed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Grade {
    pub id: i64,
    pub course_id: String,
    pub brightspace_id: Option<String>,
    pub name: String,
    pub displayed_grade: Option<String>,
    pub numerator: Option<f64>,
    pub denominator: Option<f64>,
    pub weight: Option<f64>,
    pub due_date: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub is_extra_credit: bool,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub hidden: bool,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub manually_marked_ungraded: bool,
    pub expected_score: Option<f64>,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GradeRow {
    #[serde(flatten)]
    pub grade: Grade,
    pub perc: Option<f64>,
    pub is_graded: bool,
    pub is_expected: bool,
    pub rel_weight: f64,
    pub submitted: Option<bool>,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct GradeStats {
    pub score: Option<f64>,
    pub confidence: f64,
    pub total_points_earned: f64,
    pub total_points_possible: f64,
    pub all_possible_points: f64,
    pub remaining_points: f64,
    pub target_grade: f64,
    pub required_avg: Option<f64>,
    pub is_impossible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Assignment {
    pub id: i64,
    pub course_id: String,
    pub brightspace_id: String,
    pub name: String,
    pub due_date: Option<String>,
    pub description: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub is_graded: bool,
    pub grade_item_id: Option<String>,
    pub assignment_type: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub completed: bool,
    pub completed_at: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub synthetic: bool,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub optional: bool,
    pub external_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Notification {
    pub id: i64,
    pub external_id: String,
    pub notification_type: String,
    pub title: String,
    pub body: Option<String>,
    pub date: Option<String>,
    pub course_id: Option<String>,
    pub course_name: Option<String>,
    pub urgency: i64,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub is_personal: bool,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub is_read: bool,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserPreferences {
    pub id: i64,
    pub display_name: Option<String>,
    pub time_zone: Option<String>,
    pub brightspace_host: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub api_enabled: bool,
    pub api_key: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub api_listen_all: bool,
    pub api_port: i64,
    pub jwt_secret: Option<String>,
    pub historic_gpa: Option<f64>,
    pub historic_units: Option<f64>,
    pub default_semester: Option<String>,
    pub brightspace_uid: Option<String>,
    pub brightspace_user_id: Option<String>,
    pub last_login_at: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub calendar_show_empty_days: bool,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub cache_content: bool,
    pub zotero_user_id: Option<String>,
    pub zotero_api_key: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub zotero_use_local: bool,
    pub zotero_local_base_url: Option<String>,
    pub zotero_local_user_id: Option<String>,
    pub zotero_basic_auth_user: Option<String>,
    pub zotero_basic_auth_pass: Option<String>,
    pub spotify_client_id: Option<String>,
    pub spotify_client_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthStatus {
    pub authenticated: bool,
    pub degraded: bool,
    pub host: Option<String>,
    pub user_id: Option<String>,
    pub uid: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncState {
    Idle,
    Syncing,
    Error,
}

impl Default for SyncState {
    fn default() -> Self { SyncState::Idle }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncStatus {
    pub status: SyncState,
    pub current_task: Option<String>,
    pub progress: f64,
    pub last_sync_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ContentModule {
    pub id: i64,
    pub course_id: String,
    pub brightspace_id: String,
    pub title: String,
    pub description: Option<String>,
    pub sort_order: Option<i64>,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ContentItem {
    pub id: i64,
    pub module_id: String,
    pub brightspace_id: String,
    pub title: String,
    pub item_type: Option<String>,
    pub url: Option<String>,
    #[serde(deserialize_with = "de_bool", serialize_with = "ser_bool")]
    pub is_hidden: bool,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DiscussionForum {
    pub id: i64,
    pub brightspace_id: String,
    pub course_id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DiscussionTopic {
    pub id: i64,
    pub brightspace_id: String,
    pub course_id: String,
    pub forum_id: String,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: Option<i64>,
    pub thread_count: Option<i64>,
    pub post_count: Option<i64>,
    pub last_post_date: Option<String>,
}

// SQLite stores booleans as INTEGER. Helpers for serde when flowing through JSON.
fn de_bool<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either { B(bool), I(i64) }
    Ok(match Either::deserialize(d)? {
        Either::B(b) => b,
        Either::I(i) => i != 0,
    })
}

fn ser_bool<S: serde::Serializer>(b: &bool, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bool(*b)
}
