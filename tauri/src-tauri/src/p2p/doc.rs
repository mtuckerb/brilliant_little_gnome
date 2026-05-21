// Loro CRDT wrapper. Schema layout in design.md §4 + field map §10.
//
// One LoroDoc per user. The top of the doc is a fixed set of named root
// containers (`prefs`, `course_overlays`, `assignment_overlays`,
// `synthetic_assignments`, `grade_overlays`, `content_item_overlays`,
// `notification_read`) — Loro identifies roots by their (name, type)
// pair, so we never "create" them; calling `doc.get_map(name)` is
// idempotent and lazy.
//
// Loro `Text` is used ONLY for the four collaborative-edit fields
// called out in design.md §4 / kickoff watch-out #5:
//   - prefs.display_name
//   - assignment_overlays.<key>.{name, description}
//   - synthetic_assignments.<uuid>.{name, description}
//   - grade_overlays.<key>.comments
// Everything else is a plain LoroValue (bool / i64 / f64 / String).
//
// Setters take a single `*Field` enum variant rather than one method
// per column; that keeps T-009's bridge `match` arms exhaustive at
// compile time.

#![allow(dead_code)]

use crate::error::{AppError, Result};
use loro::{Container, LoroDoc, LoroMap, LoroText, LoroValue, UpdateOptions, ValueOrContainer};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ---- Read views (returned from getters / iterators) -----------------------

// Serialize/Deserialize on the overlay structs is for the
// `pending_overlay_apply` JSON payload (T-011). `default` on missing
// fields lets us evolve the schema without breaking older payloads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CourseOverlay {
    pub is_pinned: Option<bool>,
    pub custom_name: Option<String>,
    pub custom_color: Option<String>,
    pub custom_code: Option<String>,
    pub units: Option<f64>,
    pub target_grade: Option<f64>,
    pub sort_order: Option<i64>,
    pub end_of_week_day: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssignmentOverlay {
    pub completed: Option<bool>,
    pub completed_at: Option<i64>,
    pub optional: Option<bool>,
    pub manually_edited: Option<bool>,
    pub manually_edited_at: Option<i64>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GradeOverlay {
    pub is_extra_credit: Option<bool>,
    pub hidden: Option<bool>,
    pub manually_marked_ungraded: Option<bool>,
    pub expected_score: Option<f64>,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContentItemOverlay {
    pub is_hidden: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyntheticAssignment {
    pub course_id: i64,
    pub name: String,
    pub description: String,
    pub due_date: Option<String>,
    pub optional: bool,
    pub completed: bool,
    pub completed_at: Option<i64>,
    pub created_at: i64,
}

// ---- Write enums (one variant per Class-B SQLite column) ------------------

#[derive(Debug, Clone)]
pub enum CourseField {
    IsPinned(bool),
    CustomName(Option<String>),
    CustomColor(Option<String>),
    CustomCode(Option<String>),
    Units(Option<f64>),
    TargetGrade(Option<f64>),
    SortOrder(Option<i64>),
    EndOfWeekDay(Option<i64>),
}

#[derive(Debug, Clone)]
pub enum AssignmentField {
    Completed(bool),
    CompletedAt(Option<i64>),
    Optional(bool),
    ManuallyEdited(bool),
    ManuallyEditedAt(Option<i64>),
    Name(String),         // Text — collaborative
    Description(String),  // Text — collaborative
    DueDate(Option<String>),
}

#[derive(Debug, Clone)]
pub enum GradeField {
    IsExtraCredit(bool),
    Hidden(bool),
    ManuallyMarkedUngraded(bool),
    ExpectedScore(Option<f64>),
    Comments(String),     // Text — collaborative
}

// ---- The wrapper ----------------------------------------------------------

pub struct SyncDoc {
    inner: Arc<LoroDoc>,
}

impl Default for SyncDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncDoc {
    pub fn new() -> Self {
        Self { inner: Arc::new(LoroDoc::new()) }
    }

    pub fn from_doc(doc: LoroDoc) -> Self {
        Self { inner: Arc::new(doc) }
    }

    /// Hand out a clone of the inner Arc. The transport / persistence
    /// layers (T-005, T-006) need to call `import` / `export` directly
    /// on the doc, but they should never own it exclusively.
    pub fn doc(&self) -> Arc<LoroDoc> {
        self.inner.clone()
    }

    fn prefs(&self) -> LoroMap { self.inner.get_map("prefs") }
    fn course_overlays(&self) -> LoroMap { self.inner.get_map("course_overlays") }
    fn assignment_overlays(&self) -> LoroMap { self.inner.get_map("assignment_overlays") }
    fn synthetic_assignments(&self) -> LoroMap { self.inner.get_map("synthetic_assignments") }
    fn grade_overlays(&self) -> LoroMap { self.inner.get_map("grade_overlays") }
    fn content_item_overlays(&self) -> LoroMap { self.inner.get_map("content_item_overlays") }
    fn notification_read(&self) -> LoroMap { self.inner.get_map("notification_read") }

    fn commit(&self) {
        self.inner.commit();
    }

    // ---- Prefs ------------------------------------------------------------

    pub fn set_pref_display_name(&self, value: &str) -> Result<()> {
        let prefs = self.prefs();
        let text = prefs.get_or_create_container("display_name", LoroText::new())?;
        update_text(&text, value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_display_name(&self) -> Option<String> {
        as_text(&self.prefs(), "display_name").map(|t| t.to_string())
    }

    pub fn set_pref_time_zone(&self, value: &str) -> Result<()> {
        self.prefs().insert("time_zone", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_time_zone(&self) -> Option<String> {
        get_string(&self.prefs(), "time_zone")
    }

    pub fn set_pref_brightspace_cookie(&self, value: &str) -> Result<()> {
        self.prefs().insert("brightspace_cookie", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_brightspace_cookie(&self) -> Option<String> {
        get_string(&self.prefs(), "brightspace_cookie")
    }

    pub fn set_pref_brightspace_host(&self, value: &str) -> Result<()> {
        self.prefs().insert("brightspace_host", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_brightspace_host(&self) -> Option<String> {
        get_string(&self.prefs(), "brightspace_host")
    }

    pub fn set_pref_historic_gpa(&self, value: f64) -> Result<()> {
        self.prefs().insert("historic_gpa", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_historic_gpa(&self) -> Option<f64> {
        get_f64(&self.prefs(), "historic_gpa")
    }

    pub fn set_pref_historic_units(&self, value: f64) -> Result<()> {
        self.prefs().insert("historic_units", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_historic_units(&self) -> Option<f64> {
        get_f64(&self.prefs(), "historic_units")
    }

    pub fn set_pref_default_semester(&self, value: &str) -> Result<()> {
        self.prefs().insert("default_semester", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_default_semester(&self) -> Option<String> {
        get_string(&self.prefs(), "default_semester")
    }

    pub fn set_pref_semester_color(&self, semester: &str, color: &str) -> Result<()> {
        let prefs = self.prefs();
        let m = prefs.get_or_create_container("semester_colors", LoroMap::new())?;
        m.insert(semester, color)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_semester_color(&self, semester: &str) -> Option<String> {
        let m = as_map(&self.prefs(), "semester_colors")?;
        get_string(&m, semester)
    }

    pub fn iter_pref_semester_colors(&self) -> Vec<(String, String)> {
        match as_map(&self.prefs(), "semester_colors") {
            Some(m) => collect_string_map(&m),
            None => Vec::new(),
        }
    }

    pub fn set_pref_collapsed_topic(&self, topic: &str, collapsed: bool) -> Result<()> {
        let prefs = self.prefs();
        let m = prefs.get_or_create_container("collapsed_topics", LoroMap::new())?;
        m.insert(topic, collapsed)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_collapsed_topic(&self, topic: &str) -> Option<bool> {
        let m = as_map(&self.prefs(), "collapsed_topics")?;
        get_bool(&m, topic)
    }

    pub fn iter_pref_collapsed_topics(&self) -> Vec<(String, bool)> {
        match as_map(&self.prefs(), "collapsed_topics") {
            Some(m) => collect_bool_map(&m),
            None => Vec::new(),
        }
    }

    pub fn set_pref_show_upcoming_assignments(&self, value: bool) -> Result<()> {
        self.prefs().insert("show_upcoming_assignments", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_show_upcoming_assignments(&self) -> Option<bool> {
        get_bool(&self.prefs(), "show_upcoming_assignments")
    }

    pub fn set_pref_show_course_list(&self, value: bool) -> Result<()> {
        self.prefs().insert("show_course_list", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_show_course_list(&self) -> Option<bool> {
        get_bool(&self.prefs(), "show_course_list")
    }

    pub fn set_pref_show_recent_updates(&self, value: bool) -> Result<()> {
        self.prefs().insert("show_recent_updates", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_show_recent_updates(&self) -> Option<bool> {
        get_bool(&self.prefs(), "show_recent_updates")
    }

    pub fn set_pref_calendar_show_empty_days(&self, value: bool) -> Result<()> {
        self.prefs().insert("calendar_show_empty_days", value)?;
        self.commit();
        Ok(())
    }

    pub fn get_pref_calendar_show_empty_days(&self) -> Option<bool> {
        get_bool(&self.prefs(), "calendar_show_empty_days")
    }

    // ---- Course overlays --------------------------------------------------

    pub fn set_course_overlay(&self, id: &str, field: CourseField) -> Result<()> {
        let parent = self.course_overlays();
        let m = parent.get_or_create_container(id, LoroMap::new())?;
        match field {
            CourseField::IsPinned(v) => m.insert("is_pinned", v)?,
            CourseField::CustomName(v) => insert_opt_string(&m, "custom_name", v.as_deref())?,
            CourseField::CustomColor(v) => insert_opt_string(&m, "custom_color", v.as_deref())?,
            CourseField::CustomCode(v) => insert_opt_string(&m, "custom_code", v.as_deref())?,
            CourseField::Units(v) => insert_opt_f64(&m, "units", v)?,
            CourseField::TargetGrade(v) => insert_opt_f64(&m, "target_grade", v)?,
            CourseField::SortOrder(v) => insert_opt_i64(&m, "sort_order", v)?,
            CourseField::EndOfWeekDay(v) => insert_opt_i64(&m, "end_of_week_day", v)?,
        }
        self.commit();
        Ok(())
    }

    pub fn get_course_overlay(&self, id: &str) -> Option<CourseOverlay> {
        let m = as_map(&self.course_overlays(), id)?;
        Some(read_course_overlay(&m))
    }

    pub fn iter_course_overlays(&self) -> Vec<(String, CourseOverlay)> {
        let parent = self.course_overlays();
        let mut out = Vec::with_capacity(parent.len());
        parent.for_each(|k, v| {
            if let ValueOrContainer::Container(Container::Map(m)) = v {
                out.push((k.to_string(), read_course_overlay(&m)));
            }
        });
        out
    }

    // ---- Assignment overlays ---------------------------------------------

    pub fn set_assignment_overlay(&self, key: &str, field: AssignmentField) -> Result<()> {
        let parent = self.assignment_overlays();
        let m = parent.get_or_create_container(key, LoroMap::new())?;
        match field {
            AssignmentField::Completed(v) => m.insert("completed", v)?,
            AssignmentField::CompletedAt(v) => insert_opt_i64(&m, "completed_at", v)?,
            AssignmentField::Optional(v) => m.insert("optional", v)?,
            AssignmentField::ManuallyEdited(v) => m.insert("manually_edited", v)?,
            AssignmentField::ManuallyEditedAt(v) => insert_opt_i64(&m, "manually_edited_at", v)?,
            AssignmentField::Name(v) => {
                let t = m.get_or_create_container("name", LoroText::new())?;
                update_text(&t, &v)?;
            }
            AssignmentField::Description(v) => {
                let t = m.get_or_create_container("description", LoroText::new())?;
                update_text(&t, &v)?;
            }
            AssignmentField::DueDate(v) => insert_opt_string(&m, "due_date", v.as_deref())?,
        }
        self.commit();
        Ok(())
    }

    pub fn get_assignment_overlay(&self, key: &str) -> Option<AssignmentOverlay> {
        let m = as_map(&self.assignment_overlays(), key)?;
        Some(read_assignment_overlay(&m))
    }

    pub fn iter_assignment_overlays(&self) -> Vec<(String, AssignmentOverlay)> {
        let parent = self.assignment_overlays();
        let mut out = Vec::with_capacity(parent.len());
        parent.for_each(|k, v| {
            if let ValueOrContainer::Container(Container::Map(m)) = v {
                out.push((k.to_string(), read_assignment_overlay(&m)));
            }
        });
        out
    }

    // ---- Synthetic assignments -------------------------------------------

    pub fn upsert_synthetic_assignment(&self, uuid: &str, a: &SyntheticAssignment) -> Result<()> {
        let parent = self.synthetic_assignments();
        let m = parent.get_or_create_container(uuid, LoroMap::new())?;
        m.insert("course_id", a.course_id)?;
        let name_text = m.get_or_create_container("name", LoroText::new())?;
        update_text(&name_text, &a.name)?;
        let desc_text = m.get_or_create_container("description", LoroText::new())?;
        update_text(&desc_text, &a.description)?;
        insert_opt_string(&m, "due_date", a.due_date.as_deref())?;
        m.insert("optional", a.optional)?;
        m.insert("completed", a.completed)?;
        insert_opt_i64(&m, "completed_at", a.completed_at)?;
        m.insert("created_at", a.created_at)?;
        self.commit();
        Ok(())
    }

    pub fn delete_synthetic_assignment(&self, uuid: &str) -> Result<()> {
        self.synthetic_assignments().delete(uuid)?;
        self.commit();
        Ok(())
    }

    pub fn get_synthetic_assignment(&self, uuid: &str) -> Option<SyntheticAssignment> {
        let m = as_map(&self.synthetic_assignments(), uuid)?;
        read_synthetic_assignment(&m)
    }

    pub fn iter_synthetic_assignments(&self) -> Vec<(String, SyntheticAssignment)> {
        let parent = self.synthetic_assignments();
        let mut out = Vec::with_capacity(parent.len());
        parent.for_each(|k, v| {
            if let ValueOrContainer::Container(Container::Map(m)) = v {
                if let Some(a) = read_synthetic_assignment(&m) {
                    out.push((k.to_string(), a));
                }
            }
        });
        out
    }

    // ---- Grade overlays --------------------------------------------------

    pub fn set_grade_overlay(&self, key: &str, field: GradeField) -> Result<()> {
        let parent = self.grade_overlays();
        let m = parent.get_or_create_container(key, LoroMap::new())?;
        match field {
            GradeField::IsExtraCredit(v) => m.insert("is_extra_credit", v)?,
            GradeField::Hidden(v) => m.insert("hidden", v)?,
            GradeField::ManuallyMarkedUngraded(v) => m.insert("manually_marked_ungraded", v)?,
            GradeField::ExpectedScore(v) => insert_opt_f64(&m, "expected_score", v)?,
            GradeField::Comments(v) => {
                let t = m.get_or_create_container("comments", LoroText::new())?;
                update_text(&t, &v)?;
            }
        }
        self.commit();
        Ok(())
    }

    pub fn get_grade_overlay(&self, key: &str) -> Option<GradeOverlay> {
        let m = as_map(&self.grade_overlays(), key)?;
        Some(read_grade_overlay(&m))
    }

    pub fn iter_grade_overlays(&self) -> Vec<(String, GradeOverlay)> {
        let parent = self.grade_overlays();
        let mut out = Vec::with_capacity(parent.len());
        parent.for_each(|k, v| {
            if let ValueOrContainer::Container(Container::Map(m)) = v {
                out.push((k.to_string(), read_grade_overlay(&m)));
            }
        });
        out
    }

    // ---- Content item overlays -------------------------------------------

    pub fn set_content_item_hidden(&self, brightspace_id: &str, hidden: bool) -> Result<()> {
        let parent = self.content_item_overlays();
        let m = parent.get_or_create_container(brightspace_id, LoroMap::new())?;
        m.insert("is_hidden", hidden)?;
        self.commit();
        Ok(())
    }

    pub fn get_content_item_overlay(&self, brightspace_id: &str) -> Option<ContentItemOverlay> {
        let m = as_map(&self.content_item_overlays(), brightspace_id)?;
        Some(ContentItemOverlay {
            is_hidden: get_bool(&m, "is_hidden"),
        })
    }

    pub fn iter_content_item_overlays(&self) -> Vec<(String, ContentItemOverlay)> {
        let parent = self.content_item_overlays();
        let mut out = Vec::with_capacity(parent.len());
        parent.for_each(|k, v| {
            if let ValueOrContainer::Container(Container::Map(m)) = v {
                out.push((
                    k.to_string(),
                    ContentItemOverlay { is_hidden: get_bool(&m, "is_hidden") },
                ));
            }
        });
        out
    }

    // ---- Notifications ---------------------------------------------------

    pub fn set_notification_read(&self, external_id: &str, read: bool) -> Result<()> {
        self.notification_read().insert(external_id, read)?;
        self.commit();
        Ok(())
    }

    pub fn get_notification_read(&self, external_id: &str) -> Option<bool> {
        get_bool(&self.notification_read(), external_id)
    }

    pub fn iter_notifications_read(&self) -> Vec<(String, bool)> {
        collect_bool_map(&self.notification_read())
    }
}

// ---- Helpers --------------------------------------------------------------

fn update_text(text: &LoroText, value: &str) -> Result<()> {
    text.update(value, UpdateOptions::default())
        .map_err(|e| AppError::Other(format!("loro text update: {}", e)))
}

fn as_text(parent: &LoroMap, key: &str) -> Option<LoroText> {
    match parent.get(key)? {
        ValueOrContainer::Container(Container::Text(t)) => Some(t),
        _ => None,
    }
}

fn as_map(parent: &LoroMap, key: &str) -> Option<LoroMap> {
    match parent.get(key)? {
        ValueOrContainer::Container(Container::Map(m)) => Some(m),
        _ => None,
    }
}

fn get_string(map: &LoroMap, key: &str) -> Option<String> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::String(s)) => {
            Some(AsRef::<str>::as_ref(&s).to_string())
        }
        _ => None,
    }
}

fn get_bool(map: &LoroMap, key: &str) -> Option<bool> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::Bool(b)) => Some(b),
        _ => None,
    }
}

fn get_i64(map: &LoroMap, key: &str) -> Option<i64> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::I64(i)) => Some(i),
        _ => None,
    }
}

fn get_f64(map: &LoroMap, key: &str) -> Option<f64> {
    match map.get(key)? {
        ValueOrContainer::Value(LoroValue::Double(d)) => Some(d),
        _ => None,
    }
}

fn get_text_string(map: &LoroMap, key: &str) -> Option<String> {
    as_text(map, key).map(|t| t.to_string())
}

/// Set Optional<String> — Some("x") writes the string; None deletes the key.
/// This matches the Class-B reset semantic (clear an override → revert to
/// the Brightspace value) described in design.md §4 "What about deletes?".
fn insert_opt_string(map: &LoroMap, key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(v) => map.insert(key, v)?,
        None => {
            // delete() returns Ok even if the key isn't present
            map.delete(key)?;
        }
    }
    Ok(())
}

fn insert_opt_i64(map: &LoroMap, key: &str, value: Option<i64>) -> Result<()> {
    match value {
        Some(v) => map.insert(key, v)?,
        None => {
            map.delete(key)?;
        }
    }
    Ok(())
}

fn insert_opt_f64(map: &LoroMap, key: &str, value: Option<f64>) -> Result<()> {
    match value {
        Some(v) => map.insert(key, v)?,
        None => {
            map.delete(key)?;
        }
    }
    Ok(())
}

fn collect_string_map(map: &LoroMap) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(map.len());
    map.for_each(|k, v| {
        if let ValueOrContainer::Value(LoroValue::String(s)) = v {
            out.push((k.to_string(), AsRef::<str>::as_ref(&s).to_string()));
        }
    });
    out
}

fn collect_bool_map(map: &LoroMap) -> Vec<(String, bool)> {
    let mut out = Vec::with_capacity(map.len());
    map.for_each(|k, v| {
        if let ValueOrContainer::Value(LoroValue::Bool(b)) = v {
            out.push((k.to_string(), b));
        }
    });
    out
}

fn read_course_overlay(m: &LoroMap) -> CourseOverlay {
    CourseOverlay {
        is_pinned: get_bool(m, "is_pinned"),
        custom_name: get_string(m, "custom_name"),
        custom_color: get_string(m, "custom_color"),
        custom_code: get_string(m, "custom_code"),
        units: get_f64(m, "units"),
        target_grade: get_f64(m, "target_grade"),
        sort_order: get_i64(m, "sort_order"),
        end_of_week_day: get_i64(m, "end_of_week_day"),
    }
}

fn read_assignment_overlay(m: &LoroMap) -> AssignmentOverlay {
    AssignmentOverlay {
        completed: get_bool(m, "completed"),
        completed_at: get_i64(m, "completed_at"),
        optional: get_bool(m, "optional"),
        manually_edited: get_bool(m, "manually_edited"),
        manually_edited_at: get_i64(m, "manually_edited_at"),
        name: get_text_string(m, "name"),
        description: get_text_string(m, "description"),
        due_date: get_string(m, "due_date"),
    }
}

fn read_grade_overlay(m: &LoroMap) -> GradeOverlay {
    GradeOverlay {
        is_extra_credit: get_bool(m, "is_extra_credit"),
        hidden: get_bool(m, "hidden"),
        manually_marked_ungraded: get_bool(m, "manually_marked_ungraded"),
        expected_score: get_f64(m, "expected_score"),
        comments: get_text_string(m, "comments"),
    }
}

fn read_synthetic_assignment(m: &LoroMap) -> Option<SyntheticAssignment> {
    Some(SyntheticAssignment {
        course_id: get_i64(m, "course_id")?,
        name: get_text_string(m, "name").unwrap_or_default(),
        description: get_text_string(m, "description").unwrap_or_default(),
        due_date: get_string(m, "due_date"),
        optional: get_bool(m, "optional").unwrap_or(false),
        completed: get_bool(m, "completed").unwrap_or(false),
        completed_at: get_i64(m, "completed_at"),
        created_at: get_i64(m, "created_at")?,
    })
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_pref_field() {
        let d = SyncDoc::new();
        d.set_pref_display_name("Tucker").unwrap();
        d.set_pref_time_zone("America/New_York").unwrap();
        d.set_pref_historic_gpa(3.74).unwrap();
        d.set_pref_historic_units(120.0).unwrap();
        d.set_pref_default_semester("2026-spring").unwrap();
        d.set_pref_semester_color("2026-spring", "#ff8800").unwrap();
        d.set_pref_semester_color("2025-fall", "#00aa44").unwrap();
        d.set_pref_collapsed_topic("topic:42", true).unwrap();
        d.set_pref_show_upcoming_assignments(true).unwrap();
        d.set_pref_show_course_list(false).unwrap();
        d.set_pref_show_recent_updates(true).unwrap();
        d.set_pref_calendar_show_empty_days(false).unwrap();

        assert_eq!(d.get_pref_display_name().as_deref(), Some("Tucker"));
        assert_eq!(d.get_pref_time_zone().as_deref(), Some("America/New_York"));
        assert_eq!(d.get_pref_historic_gpa(), Some(3.74));
        assert_eq!(d.get_pref_historic_units(), Some(120.0));
        assert_eq!(d.get_pref_default_semester().as_deref(), Some("2026-spring"));
        assert_eq!(d.get_pref_semester_color("2026-spring").as_deref(), Some("#ff8800"));
        assert_eq!(d.get_pref_semester_color("2025-fall").as_deref(), Some("#00aa44"));
        let mut colors = d.iter_pref_semester_colors();
        colors.sort();
        assert_eq!(colors.len(), 2);
        assert_eq!(d.get_pref_collapsed_topic("topic:42"), Some(true));
        assert_eq!(d.get_pref_show_upcoming_assignments(), Some(true));
        assert_eq!(d.get_pref_show_course_list(), Some(false));
        assert_eq!(d.get_pref_show_recent_updates(), Some(true));
        assert_eq!(d.get_pref_calendar_show_empty_days(), Some(false));
    }

    #[test]
    fn round_trips_course_overlay_fields() {
        let d = SyncDoc::new();
        let id = "12345";
        d.set_course_overlay(id, CourseField::IsPinned(true)).unwrap();
        d.set_course_overlay(id, CourseField::CustomName(Some("Intro to Psychology".into()))).unwrap();
        d.set_course_overlay(id, CourseField::CustomColor(Some("#abcdef".into()))).unwrap();
        d.set_course_overlay(id, CourseField::CustomName(Some("Calculus I".into()))).unwrap();
        d.set_course_overlay(id, CourseField::CustomCode(Some("MAT-101".into()))).unwrap();
        d.set_course_overlay(id, CourseField::Units(Some(3.0))).unwrap();
        d.set_course_overlay(id, CourseField::TargetGrade(Some(95.0))).unwrap();
        d.set_course_overlay(id, CourseField::SortOrder(Some(7))).unwrap();
        d.set_course_overlay(id, CourseField::EndOfWeekDay(Some(5))).unwrap();

        let got = d.get_course_overlay(id).unwrap();
        assert_eq!(got, CourseOverlay {
            is_pinned: Some(true),
            custom_name: Some("Calculus I".into()),
            custom_color: Some("#abcdef".into()),
            custom_code: Some("MAT-101".into()),
            units: Some(3.0),
            target_grade: Some(95.0),
            sort_order: Some(7),
            end_of_week_day: Some(5),
        });

        // None on an Option<_> field clears it (overlay reset semantic).
        d.set_course_overlay(id, CourseField::CustomColor(None)).unwrap();
        assert_eq!(d.get_course_overlay(id).unwrap().custom_color, None);
        d.set_course_overlay(id, CourseField::CustomName(None)).unwrap();
        assert_eq!(d.get_course_overlay(id).unwrap().custom_name, None);
        d.set_course_overlay(id, CourseField::CustomCode(None)).unwrap();
        assert_eq!(d.get_course_overlay(id).unwrap().custom_code, None);

        let all = d.iter_course_overlays();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, id);
    }

    #[test]
    fn round_trips_assignment_overlay_fields() {
        let d = SyncDoc::new();
        let key = "100:9999";
        d.set_assignment_overlay(key, AssignmentField::Completed(true)).unwrap();
        d.set_assignment_overlay(key, AssignmentField::CompletedAt(Some(1700000000))).unwrap();
        d.set_assignment_overlay(key, AssignmentField::Optional(false)).unwrap();
        d.set_assignment_overlay(key, AssignmentField::ManuallyEdited(true)).unwrap();
        d.set_assignment_overlay(key, AssignmentField::ManuallyEditedAt(Some(1700000001))).unwrap();
        d.set_assignment_overlay(key, AssignmentField::Name("Edited title".into())).unwrap();
        d.set_assignment_overlay(key, AssignmentField::Description("Edited body".into())).unwrap();
        d.set_assignment_overlay(key, AssignmentField::DueDate(Some("2026-05-01".into()))).unwrap();

        let got = d.get_assignment_overlay(key).unwrap();
        assert_eq!(got, AssignmentOverlay {
            completed: Some(true),
            completed_at: Some(1700000000),
            optional: Some(false),
            manually_edited: Some(true),
            manually_edited_at: Some(1700000001),
            name: Some("Edited title".into()),
            description: Some("Edited body".into()),
            due_date: Some("2026-05-01".into()),
        });

        // Edit the Text field — Loro Text update should converge, not clobber.
        d.set_assignment_overlay(key, AssignmentField::Description("Edited body, then more".into()))
            .unwrap();
        assert_eq!(
            d.get_assignment_overlay(key).unwrap().description.as_deref(),
            Some("Edited body, then more"),
        );
    }

    #[test]
    fn round_trips_synthetic_assignment() {
        let d = SyncDoc::new();
        let uuid = "00000000-0000-4000-8000-000000000001";
        let a = SyntheticAssignment {
            course_id: 42,
            name: "My self-imposed deadline".into(),
            description: "Notes".into(),
            due_date: Some("2026-05-15".into()),
            optional: false,
            completed: false,
            completed_at: None,
            created_at: 1700000000,
        };
        d.upsert_synthetic_assignment(uuid, &a).unwrap();
        assert_eq!(d.get_synthetic_assignment(uuid).as_ref(), Some(&a));

        // Mutate via upsert (e.g., mark complete).
        let a2 = SyntheticAssignment {
            completed: true,
            completed_at: Some(1700100000),
            ..a.clone()
        };
        d.upsert_synthetic_assignment(uuid, &a2).unwrap();
        assert_eq!(d.get_synthetic_assignment(uuid).unwrap(), a2);

        d.delete_synthetic_assignment(uuid).unwrap();
        assert_eq!(d.get_synthetic_assignment(uuid), None);
        assert!(d.iter_synthetic_assignments().is_empty());
    }

    #[test]
    fn round_trips_grade_overlay_fields() {
        let d = SyncDoc::new();
        let key = "100:5555";
        d.set_grade_overlay(key, GradeField::IsExtraCredit(true)).unwrap();
        d.set_grade_overlay(key, GradeField::Hidden(false)).unwrap();
        d.set_grade_overlay(key, GradeField::ManuallyMarkedUngraded(true)).unwrap();
        d.set_grade_overlay(key, GradeField::ExpectedScore(Some(92.5))).unwrap();
        d.set_grade_overlay(key, GradeField::Comments("Great work!".into())).unwrap();

        let got = d.get_grade_overlay(key).unwrap();
        assert_eq!(got, GradeOverlay {
            is_extra_credit: Some(true),
            hidden: Some(false),
            manually_marked_ungraded: Some(true),
            expected_score: Some(92.5),
            comments: Some("Great work!".into()),
        });
    }

    #[test]
    fn round_trips_content_item_and_notification() {
        let d = SyncDoc::new();
        d.set_content_item_hidden("ci-1", true).unwrap();
        d.set_content_item_hidden("ci-2", false).unwrap();
        assert_eq!(d.get_content_item_overlay("ci-1").unwrap().is_hidden, Some(true));
        assert_eq!(d.get_content_item_overlay("ci-2").unwrap().is_hidden, Some(false));
        assert_eq!(d.iter_content_item_overlays().len(), 2);

        d.set_notification_read("ext-1", true).unwrap();
        d.set_notification_read("ext-2", false).unwrap();
        assert_eq!(d.get_notification_read("ext-1"), Some(true));
        assert_eq!(d.get_notification_read("ext-2"), Some(false));
        let mut reads = d.iter_notifications_read();
        reads.sort();
        assert_eq!(reads, vec![("ext-1".into(), true), ("ext-2".into(), false)]);
    }

    #[test]
    fn two_docs_merge_concurrent_text_edits() {
        // Concurrent edits to the same Text field on two devices should
        // merge — this is the whole reason Loro Text exists in the schema.
        let a = SyncDoc::new();
        let b = SyncDoc::new();
        a.set_pref_display_name("Hello").unwrap();
        // Sync initial state into b.
        let snap = a.doc().export(loro::ExportMode::Snapshot).unwrap();
        b.doc().import(&snap).unwrap();
        assert_eq!(b.get_pref_display_name().as_deref(), Some("Hello"));

        // Both devices edit independently.
        a.set_pref_display_name("Hello world").unwrap();
        b.set_pref_display_name("Hello there").unwrap();

        // Exchange updates.
        let from_a = a.doc().export(loro::ExportMode::all_updates()).unwrap();
        let from_b = b.doc().export(loro::ExportMode::all_updates()).unwrap();
        a.doc().import(&from_b).unwrap();
        b.doc().import(&from_a).unwrap();

        // Both converge to the same value (Loro picks a deterministic merge).
        assert_eq!(a.get_pref_display_name(), b.get_pref_display_name());
        let merged = a.get_pref_display_name().unwrap();
        // The merged value contains characters from both edits — exact form
        // depends on Loro's RGA tiebreak, but it's not just one or the other
        // verbatim.
        assert!(merged.contains("Hello"));
    }
}
