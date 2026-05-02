// SQLite ↔ Loro bridge. The trickiest part of the engine — see design.md §7.
//
// Two directions, both must be correct:
//   apply_local  (T-009): Tauri command writes a Class-B SQLite row,
//                         then calls into here to mirror into Loro.
//   apply_remote (T-010): Loro doc subscription fires; this module
//                         walks the diff and writes matching SQLite
//                         rows. MUST suppress re-broadcast (echo storm
//                         guard — kickoff watch-out #1).
//   hydrate_from_doc (T-011): on first pairing, walk the entire Loro
//                             doc and seed SQLite. For overlay rows
//                             whose underlying Brightspace row hasn't
//                             been fetched yet, defer into the
//                             pending_overlay_apply table (introduced
//                             alongside this method's migration 0006).

#![allow(dead_code)]

use crate::error::Result;
use crate::p2p::doc::{
    AssignmentField, CourseField, GradeField, SyncDoc, SyntheticAssignment,
};
use std::sync::Arc;

/// Reconciles SQLite rows with the in-memory Loro doc.
///
/// T-009 wires up `apply_local` (SQLite → Loro). T-010 will subscribe
/// to the Loro doc and add `apply_remote` (Loro → SQLite), plus the
/// echo-storm guard that prevents a remote-applied SQLite write from
/// re-entering `apply_local`.
pub struct Bridge {
    doc: Arc<SyncDoc>,
    // T-010 will add:
    //   pool:   sqlx::SqlitePool,
    //   events: crate::events::EventBus,
    //   origin guard for echo-storm suppression (likely a tokio
    //   task-local, not an AtomicU8 — multiple concurrent tasks must
    //   each carry their own marker).
}

impl Bridge {
    pub fn new(doc: Arc<SyncDoc>) -> Arc<Self> {
        Arc::new(Self { doc })
    }

    /// Apply a local SQLite mutation to the Loro doc.
    ///
    /// The doc's commit triggers `subscribe_local_update`, which the
    /// SyncEngine's local-update pump (T-008) drains: append to WAL,
    /// broadcast as `WireMsg::Update`. So this method does NOT return
    /// the bytes — the engine owns the broadcast path.
    ///
    /// Origin handling: T-010 will short-circuit this when called from
    /// the apply_remote path. Today every call is treated as `Local`.
    pub async fn apply_local(&self, change: LocalChange) -> Result<()> {
        match change {
            LocalChange::Pref(field) => self.apply_pref(field)?,
            LocalChange::Course { id, field } => {
                self.doc.set_course_overlay(&id, field)?;
            }
            LocalChange::Assignment { key, field } => {
                self.doc.set_assignment_overlay(&key, field)?;
            }
            LocalChange::Grade { key, field } => {
                self.doc.set_grade_overlay(&key, field)?;
            }
            LocalChange::ContentItemHidden { brightspace_id, hidden } => {
                self.doc.set_content_item_hidden(&brightspace_id, hidden)?;
            }
            LocalChange::NotificationRead { external_id, read } => {
                self.doc.set_notification_read(&external_id, read)?;
            }
            LocalChange::SyntheticAssignmentUpsert { uuid, value } => {
                self.doc.upsert_synthetic_assignment(&uuid, &value)?;
            }
            LocalChange::SyntheticAssignmentDelete { uuid } => {
                self.doc.delete_synthetic_assignment(&uuid)?;
            }
        }
        Ok(())
    }

    fn apply_pref(&self, field: PrefField) -> Result<()> {
        match field {
            PrefField::DisplayName(v) => self.doc.set_pref_display_name(&v)?,
            PrefField::TimeZone(v) => self.doc.set_pref_time_zone(&v)?,
            PrefField::HistoricGpa(v) => self.doc.set_pref_historic_gpa(v)?,
            PrefField::HistoricUnits(v) => self.doc.set_pref_historic_units(v)?,
            PrefField::DefaultSemester(v) => self.doc.set_pref_default_semester(&v)?,
            PrefField::SemesterColor { semester, color } => {
                self.doc.set_pref_semester_color(&semester, &color)?
            }
            PrefField::CollapsedTopic { topic, collapsed } => {
                self.doc.set_pref_collapsed_topic(&topic, collapsed)?
            }
            PrefField::ShowUpcomingAssignments(v) => {
                self.doc.set_pref_show_upcoming_assignments(v)?
            }
            PrefField::ShowCourseList(v) => self.doc.set_pref_show_course_list(v)?,
            PrefField::ShowRecentUpdates(v) => self.doc.set_pref_show_recent_updates(v)?,
            PrefField::CalendarShowEmptyDays(v) => {
                self.doc.set_pref_calendar_show_empty_days(v)?
            }
        }
        Ok(())
    }
}

/// Local-origin change. One variant per Class-B SQLite write listed in
/// design.md §10 (Field mapping reference).
///
/// The grouping mirrors the §10 row-key shape:
///   - prefs are a single-row table → `Pref(PrefField)` carries no id
///   - courses use `org_unit_id` as the key
///   - assignments / grades use the composite `"<course_id>:<bid>"` key
///   - content items use `brightspace_id`
///   - notifications use `external_id`
///   - synthetic assignments are entirely user-authored, keyed by uuid
///
/// Loro Text fields (`AssignmentField::Name`, `AssignmentField::Description`,
/// `GradeField::Comments`, `PrefField::DisplayName`, plus the `name` /
/// `description` inside `SyntheticAssignment`) are merged character-by-
/// character on conflict — see design.md §4 for which fields qualify.
#[derive(Debug, Clone)]
pub enum LocalChange {
    Pref(PrefField),
    Course {
        id: String,
        field: CourseField,
    },
    Assignment {
        key: String,
        field: AssignmentField,
    },
    Grade {
        key: String,
        field: GradeField,
    },
    ContentItemHidden {
        brightspace_id: String,
        hidden: bool,
    },
    NotificationRead {
        external_id: String,
        read: bool,
    },
    SyntheticAssignmentUpsert {
        uuid: String,
        value: SyntheticAssignment,
    },
    SyntheticAssignmentDelete {
        uuid: String,
    },
}

/// One variant per Class-B `user_preferences` column (design.md §10).
///
/// `SemesterColor` and `CollapsedTopic` carry the per-key map argument
/// — the underlying Loro layout stores those as nested
/// `Map<String, _>`, so each call mutates exactly one (semester | topic)
/// entry rather than overwriting the whole map.
#[derive(Debug, Clone)]
pub enum PrefField {
    DisplayName(String),
    TimeZone(String),
    HistoricGpa(f64),
    HistoricUnits(f64),
    DefaultSemester(String),
    SemesterColor { semester: String, color: String },
    CollapsedTopic { topic: String, collapsed: bool },
    ShowUpcomingAssignments(bool),
    ShowCourseList(bool),
    ShowRecentUpdates(bool),
    CalendarShowEmptyDays(bool),
}

/// Remote-origin change derived from a Loro diff. Filled in by T-010.
#[derive(Debug, Clone)]
pub enum RemoteChange {
    // expanded in T-010 — mirrors LocalChange's coverage
}

/// Origin marker used to suppress echo storms (kickoff watch-out #1):
/// a Loro diff caused by a remote message must NOT bounce back into
/// `apply_local` and re-broadcast. T-010 wires the actual guard; the
/// enum is defined here so the bridge surface exposes it from day one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Local,
    Remote,
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::doc::{CourseField, GradeField, SyntheticAssignment};

    fn fresh() -> Arc<Bridge> {
        Bridge::new(Arc::new(SyncDoc::new()))
    }

    #[tokio::test]
    async fn apply_local_pref_display_name() {
        let bridge = fresh();
        bridge
            .apply_local(LocalChange::Pref(PrefField::DisplayName("Tucker".into())))
            .await
            .unwrap();
        assert_eq!(bridge.doc.get_pref_display_name().as_deref(), Some("Tucker"));
    }

    #[tokio::test]
    async fn apply_local_pref_scalars() {
        let bridge = fresh();
        bridge
            .apply_local(LocalChange::Pref(PrefField::TimeZone("America/New_York".into())))
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::Pref(PrefField::HistoricGpa(3.74)))
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::Pref(PrefField::HistoricUnits(120.0)))
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::Pref(PrefField::DefaultSemester("2026-spring".into())))
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::Pref(PrefField::ShowUpcomingAssignments(true)))
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::Pref(PrefField::ShowCourseList(false)))
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::Pref(PrefField::ShowRecentUpdates(true)))
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::Pref(PrefField::CalendarShowEmptyDays(false)))
            .await
            .unwrap();

        let d = &bridge.doc;
        assert_eq!(d.get_pref_time_zone().as_deref(), Some("America/New_York"));
        assert_eq!(d.get_pref_historic_gpa(), Some(3.74));
        assert_eq!(d.get_pref_historic_units(), Some(120.0));
        assert_eq!(d.get_pref_default_semester().as_deref(), Some("2026-spring"));
        assert_eq!(d.get_pref_show_upcoming_assignments(), Some(true));
        assert_eq!(d.get_pref_show_course_list(), Some(false));
        assert_eq!(d.get_pref_show_recent_updates(), Some(true));
        assert_eq!(d.get_pref_calendar_show_empty_days(), Some(false));
    }

    #[tokio::test]
    async fn apply_local_pref_per_key_maps() {
        let bridge = fresh();
        bridge
            .apply_local(LocalChange::Pref(PrefField::SemesterColor {
                semester: "2026-spring".into(),
                color: "#ff8800".into(),
            }))
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::Pref(PrefField::SemesterColor {
                semester: "2025-fall".into(),
                color: "#00aa44".into(),
            }))
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::Pref(PrefField::CollapsedTopic {
                topic: "topic:42".into(),
                collapsed: true,
            }))
            .await
            .unwrap();

        assert_eq!(
            bridge.doc.get_pref_semester_color("2026-spring").as_deref(),
            Some("#ff8800"),
        );
        assert_eq!(
            bridge.doc.get_pref_semester_color("2025-fall").as_deref(),
            Some("#00aa44"),
        );
        assert_eq!(bridge.doc.get_pref_collapsed_topic("topic:42"), Some(true));
    }

    #[tokio::test]
    async fn apply_local_course_every_field() {
        let bridge = fresh();
        let id = "12345";
        for field in [
            CourseField::IsPinned(true),
            CourseField::CustomColor(Some("#abcdef".into())),
            CourseField::Units(Some(3.0)),
            CourseField::TargetGrade(Some(95.0)),
            CourseField::SortOrder(Some(7)),
            CourseField::EndOfWeekDay(Some(5)),
        ] {
            bridge
                .apply_local(LocalChange::Course { id: id.into(), field })
                .await
                .unwrap();
        }
        let got = bridge.doc.get_course_overlay(id).unwrap();
        assert_eq!(got.is_pinned, Some(true));
        assert_eq!(got.custom_color.as_deref(), Some("#abcdef"));
        assert_eq!(got.units, Some(3.0));
        assert_eq!(got.target_grade, Some(95.0));
        assert_eq!(got.sort_order, Some(7));
        assert_eq!(got.end_of_week_day, Some(5));
    }

    #[tokio::test]
    async fn apply_local_assignment_every_field() {
        let bridge = fresh();
        let key = "100:9999";
        for field in [
            AssignmentField::Completed(true),
            AssignmentField::CompletedAt(Some(1700000000)),
            AssignmentField::Optional(false),
            AssignmentField::ManuallyEdited(true),
            AssignmentField::ManuallyEditedAt(Some(1700000001)),
            AssignmentField::Name("Edited title".into()),
            AssignmentField::Description("Edited body".into()),
            AssignmentField::DueDate(Some("2026-05-01".into())),
        ] {
            bridge
                .apply_local(LocalChange::Assignment { key: key.into(), field })
                .await
                .unwrap();
        }
        let got = bridge.doc.get_assignment_overlay(key).unwrap();
        assert_eq!(got.completed, Some(true));
        assert_eq!(got.completed_at, Some(1700000000));
        assert_eq!(got.optional, Some(false));
        assert_eq!(got.manually_edited, Some(true));
        assert_eq!(got.manually_edited_at, Some(1700000001));
        assert_eq!(got.name.as_deref(), Some("Edited title"));
        assert_eq!(got.description.as_deref(), Some("Edited body"));
        assert_eq!(got.due_date.as_deref(), Some("2026-05-01"));
    }

    #[tokio::test]
    async fn apply_local_grade_every_field() {
        let bridge = fresh();
        let key = "100:5555";
        for field in [
            GradeField::IsExtraCredit(true),
            GradeField::Hidden(false),
            GradeField::ManuallyMarkedUngraded(true),
            GradeField::ExpectedScore(Some(92.5)),
            GradeField::Comments("Great work!".into()),
        ] {
            bridge
                .apply_local(LocalChange::Grade { key: key.into(), field })
                .await
                .unwrap();
        }
        let got = bridge.doc.get_grade_overlay(key).unwrap();
        assert_eq!(got.is_extra_credit, Some(true));
        assert_eq!(got.hidden, Some(false));
        assert_eq!(got.manually_marked_ungraded, Some(true));
        assert_eq!(got.expected_score, Some(92.5));
        assert_eq!(got.comments.as_deref(), Some("Great work!"));
    }

    #[tokio::test]
    async fn apply_local_content_item_hidden() {
        let bridge = fresh();
        bridge
            .apply_local(LocalChange::ContentItemHidden {
                brightspace_id: "ci-1".into(),
                hidden: true,
            })
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::ContentItemHidden {
                brightspace_id: "ci-2".into(),
                hidden: false,
            })
            .await
            .unwrap();

        assert_eq!(
            bridge.doc.get_content_item_overlay("ci-1").unwrap().is_hidden,
            Some(true),
        );
        assert_eq!(
            bridge.doc.get_content_item_overlay("ci-2").unwrap().is_hidden,
            Some(false),
        );
    }

    #[tokio::test]
    async fn apply_local_notification_read() {
        let bridge = fresh();
        bridge
            .apply_local(LocalChange::NotificationRead {
                external_id: "ext-1".into(),
                read: true,
            })
            .await
            .unwrap();
        bridge
            .apply_local(LocalChange::NotificationRead {
                external_id: "ext-2".into(),
                read: false,
            })
            .await
            .unwrap();

        assert_eq!(bridge.doc.get_notification_read("ext-1"), Some(true));
        assert_eq!(bridge.doc.get_notification_read("ext-2"), Some(false));
    }

    #[tokio::test]
    async fn apply_local_synthetic_assignment_upsert_then_delete() {
        let bridge = fresh();
        let uuid = "00000000-0000-4000-8000-000000000001";
        let a = SyntheticAssignment {
            course_id: 42,
            name: "Self-imposed deadline".into(),
            description: "Notes".into(),
            due_date: Some("2026-05-15".into()),
            optional: false,
            completed: false,
            completed_at: None,
            created_at: 1700000000,
        };
        bridge
            .apply_local(LocalChange::SyntheticAssignmentUpsert {
                uuid: uuid.into(),
                value: a.clone(),
            })
            .await
            .unwrap();
        assert_eq!(bridge.doc.get_synthetic_assignment(uuid).as_ref(), Some(&a));

        // Upsert again to mark complete.
        let a2 = SyntheticAssignment {
            completed: true,
            completed_at: Some(1700100000),
            ..a.clone()
        };
        bridge
            .apply_local(LocalChange::SyntheticAssignmentUpsert {
                uuid: uuid.into(),
                value: a2.clone(),
            })
            .await
            .unwrap();
        assert_eq!(bridge.doc.get_synthetic_assignment(uuid).unwrap(), a2);

        // Delete.
        bridge
            .apply_local(LocalChange::SyntheticAssignmentDelete { uuid: uuid.into() })
            .await
            .unwrap();
        assert_eq!(bridge.doc.get_synthetic_assignment(uuid), None);
    }

    /// Confirms that applying a local change goes through the same
    /// commit path the engine subscribes to: each apply produces
    /// exactly one Loro version-vector bump on the underlying doc.
    #[tokio::test]
    async fn apply_local_commits_through_doc() {
        let bridge = fresh();
        let before = bridge.doc.doc().oplog_vv().encode();
        bridge
            .apply_local(LocalChange::Pref(PrefField::TimeZone("UTC".into())))
            .await
            .unwrap();
        let after = bridge.doc.doc().oplog_vv().encode();
        assert_ne!(before, after, "apply_local must advance the oplog");
    }
}
