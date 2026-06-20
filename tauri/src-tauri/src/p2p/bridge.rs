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
//
// ---- Echo-storm guard (apply_remote) -------------------------------------
//
// The natural chokepoint is `EventTriggerKind`: Loro tags every diff event
// with `Local` (this doc's own commit), `Import` (`doc.import(...)` called),
// or `Checkout` (manual time-travel). The bridge subscribes via
// `subscribe_root` and SHORT-CIRCUITS on anything that isn't `Import`.
//
// That single filter prevents the cycle:
//   • Local apply_local() → Loro commit → diff fires with `Local` → SKIPPED
//     (the SQLite row that started the change is already authoritative)
//   • Remote import       → diff fires with `Import` → SQLite UPDATE +
//     Tauri event emit → DOES NOT call apply_local, so no re-broadcast
//
// The broadcast path itself is independently guarded inside SyncEngine:
// the `subscribe_local_update` callback that drives the gossip Update
// only fires for local commits, never for `import()`.

#![allow(dead_code)]

use crate::error::Result;
use crate::events::EventBus;
use crate::p2p::doc::{
    AssignmentField, AssignmentOverlay, ContentItemOverlay, CourseField, CourseOverlay,
    GradeField, GradeOverlay, SyncDoc, SyntheticAssignment,
};
use loro::event::Diff;
use loro::{Index, Subscription};
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::task::AbortOnDropHandle;
use tracing::warn;

/// Lets the bridge emit Tauri events without depending on the full
/// `tauri::AppHandle` — tests pass a recording impl. Production wiring
/// uses `EventBus`, which mirrors these onto the React side.
pub trait BridgeEventSink: Send + Sync {
    fn course_updated(&self, course_id: &str);
    fn assignments_updated(&self);
    fn grades_updated(&self);
    fn notifications_updated(&self);
    fn prefs_updated(&self);
    /// Reserved for T-022 storage warnings. Default no-op so test
    /// recording impls don't have to mock it.
    fn p2p_warning(&self, _message: &str) {}
}

impl BridgeEventSink for EventBus {
    fn course_updated(&self, course_id: &str) {
        EventBus::course_updated(self, course_id);
    }
    fn assignments_updated(&self) {
        EventBus::assignments_updated(self);
    }
    fn grades_updated(&self) {
        EventBus::grades_updated(self);
    }
    fn notifications_updated(&self) {
        EventBus::notifications_updated(self);
    }
    fn prefs_updated(&self) {
        EventBus::prefs_updated(self);
    }
    fn p2p_warning(&self, message: &str) {
        EventBus::p2p_warning(self, message);
    }
}

/// Reconciles SQLite rows with the in-memory Loro doc.
///
/// Two construction paths:
///  - `Bridge::new(doc)` — apply_local only; no SQLite, no event emission,
///    no diff subscription. Used by SyncEngine tests that don't exercise
///    the SQLite mirror, and by anything that just wants a Loro façade.
///  - `Bridge::with_sql(doc, pool, events)` — full wiring. Installs the
///    `subscribe_root` handler that routes remote diffs to SQLite UPDATE
///    + Tauri event emit. This is what `SyncEngine::start()` uses in
///    production.
pub struct Bridge {
    doc: Arc<SyncDoc>,
    pool: Option<SqlitePool>,
    events: Option<Arc<dyn BridgeEventSink>>,
    // Optional reference to the Brightspace client. Only used by the
    // joiner-side `BootstrapCredentials` path in the engine inbox, so
    // tests that build a bare `Bridge::new(doc)` can leave it None
    // without affecting Loro replication behaviour.
    client: parking_lot::RwLock<Option<Arc<crate::client::BrightspaceClient>>>,
    // Pauses the apply_remote diff callback while `hydrate_from_doc`
    // is walking the doc. Without this, a peer's gossip Update can
    // race with hydrate and apply twice (or out of order). The flag
    // is set/cleared via a Drop guard so panics can't strand it.
    paused: Arc<AtomicBool>,

    // Holding the Subscription keeps the diff callback registered;
    // dropping the Bridge drops the subscription. The pump task is on
    // an AbortOnDropHandle for the same reason.
    _sub: Option<Subscription>,
    _pump: Option<AbortOnDropHandle<()>>,
}

impl Bridge {
    /// Bare bridge: apply_local only, no SQLite, no events. Mostly used
    /// by engine.rs's T-008 integration tests, which exercise gossip +
    /// persistence without bringing up a SQLite pool.
    pub fn new(doc: Arc<SyncDoc>) -> Arc<Self> {
        Arc::new(Self {
            doc,
            pool: None,
            events: None,
            client: parking_lot::RwLock::new(None),
            paused: Arc::new(AtomicBool::new(false)),
            _sub: None,
            _pump: None,
        })
    }

    /// Production wiring. Subscribes to `doc.subscribe_root`, filters to
    /// `Import` events, classifies the affected entities, and streams
    /// them onto an mpsc that an async task drains into SQLite.
    ///
    /// The synchronous diff callback runs on Loro's commit thread — we
    /// MUST NOT call sqlx from there. `collect_touched` is cheap and
    /// allocation-light, then the bytes go through an unbounded channel
    /// to the async pump. Unbounded is fine: peer updates arrive at
    /// human pace (and even a burst of 100 updates costs ~kB of channel
    /// memory).
    pub fn with_sql(
        doc: Arc<SyncDoc>,
        pool: SqlitePool,
        events: Arc<dyn BridgeEventSink>,
    ) -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<HashSet<Touched>>();
        let paused = Arc::new(AtomicBool::new(false));

        let sub = doc.doc().subscribe_root(Arc::new({
            let paused = paused.clone();
            move |ev| {
                // The single guard rail. Local commits skip; the SQLite row
                // is already the source of truth. Import comes from a peer.
                // Checkout (time-travel) we ignore — we never use it.
                if !ev.triggered_by.is_import() {
                    return;
                }
                // Hydration in progress — let `hydrate_from_doc` walk the
                // doc without racing the pump.
                if paused.load(Ordering::Acquire) {
                    return;
                }
                let touched = collect_touched(&ev);
                if !touched.is_empty() {
                    let _ = tx.send(touched);
                }
            }
        }));

        let pump = AbortOnDropHandle::new(tokio::spawn({
            let doc = doc.clone();
            let pool = pool.clone();
            let events = events.clone();
            async move {
                while let Some(set) = rx.recv().await {
                    apply_touched(&doc, &pool, events.as_ref(), set).await;
                }
            }
        }));

        Arc::new(Self {
            doc,
            pool: Some(pool),
            events: Some(events),
            client: parking_lot::RwLock::new(None),
            paused,
            _sub: Some(sub),
            _pump: Some(pump),
        })
    }

    /// Inject the Brightspace `Client` so the engine's inbox handler can
    /// persist credentials received via `WireMsg::BootstrapCredentials`.
    /// Production callers (SyncEngine::start_with_bootstrap) wire this
    /// immediately after construction; tests skip it.
    pub fn set_client(&self, client: Arc<crate::client::BrightspaceClient>) {
        *self.client.write() = Some(client);
    }

    /// Borrow the configured client. Returns `None` for tests built via
    /// `Bridge::new(doc)` or when production hasn't called set_client yet.
    pub fn client(&self) -> Option<Arc<crate::client::BrightspaceClient>> {
        self.client.read().clone()
    }

    /// Borrow the SQLite pool the bridge writes through, if any.
    /// Returns `None` for a bare `Bridge::new(doc)` — used by the
    /// engine's inbox handler to gate side effects (PairingRequest's
    /// consume_nonce step) on production wiring.
    pub fn pool(&self) -> Option<&SqlitePool> {
        self.pool.as_ref()
    }

    /// Borrow the event sink, if any. Used by the engine's
    /// checkpoint task to emit `p2p:warning` for oversized
    /// snapshots (T-022).
    pub fn events(&self) -> Option<&Arc<dyn BridgeEventSink>> {
        self.events.as_ref()
    }

    /// Initial-pair hydration (T-011). Walks every Loro overlay and
    /// writes the matching SQLite row. Composite-key overlays whose
    /// Brightspace row hasn't been fetched yet are deferred into the
    /// `pending_overlay_apply` table; the next sync/*.rs fetcher to
    /// upsert that row calls `drain_pending_overlay` to re-apply.
    ///
    /// Pauses the apply_remote pump for the duration of the walk so a
    /// concurrent peer Update can't half-apply on top of the
    /// hydration. The pause is released on drop of `_g`, so a panic
    /// here can't strand the bridge in a paused state.
    ///
    /// No-ops on a Bridge built via `Bridge::new(doc)` — without a
    /// pool there's nothing to hydrate to. Safe to call repeatedly;
    /// running it after the row exists just runs `UPDATE` over the
    /// same overlay, which is idempotent.
    pub async fn hydrate_from_doc(&self) -> Result<()> {
        let Some(pool) = self.pool.as_ref() else { return Ok(()) };
        let _g = PauseGuard::new(&self.paused);

        // prefs is always present: the migration seeds the singleton row.
        write_prefs_to_sqlite(&self.doc, pool).await?;

        for (id, overlay) in self.doc.iter_course_overlays() {
            apply_or_defer_course(pool, &id, &overlay).await?;
        }
        for (key, overlay) in self.doc.iter_assignment_overlays() {
            apply_or_defer_assignment(pool, &key, &overlay).await?;
        }
        for (key, overlay) in self.doc.iter_grade_overlays() {
            apply_or_defer_grade(pool, &key, &overlay).await?;
        }
        for (bid, overlay) in self.doc.iter_content_item_overlays() {
            apply_or_defer_content_item(pool, &bid, &overlay).await?;
        }
        for (eid, read) in self.doc.iter_notifications_read() {
            apply_or_defer_notification(pool, &eid, read).await?;
        }
        // Synthetic assignments are entirely user-authored — no parent
        // Brightspace row to wait for, INSERT OR REPLACE always wins.
        for (uuid, _) in self.doc.iter_synthetic_assignments() {
            apply_synthetic_to_sqlite(&self.doc, pool, &uuid).await?;
        }
        Ok(())
    }

    /// Apply a local SQLite mutation to the Loro doc.
    ///
    /// The doc's commit triggers `subscribe_local_update`, which the
    /// SyncEngine's local-update pump (T-008) drains: append to WAL,
    /// broadcast as `WireMsg::Update`. So this method does NOT return
    /// the bytes — the engine owns the broadcast path.
    ///
    /// Safe to call concurrently with the apply_remote path: the diff
    /// it triggers is `EventTriggerKind::Local`, which the apply_remote
    /// subscription filters out (see module-level comment).
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
            PrefField::BrightspaceCookie(v) => self.doc.set_pref_brightspace_cookie(&v)?,
            PrefField::BrightspaceHost(v) => self.doc.set_pref_brightspace_host(&v)?,
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
    /// Brightspace auth cookie. Shared across paired devices so when one
    /// device re-authenticates, the others automatically pick up the new
    /// session — keeps the aggregate session lifetime as long as possible
    /// instead of every device degrading independently.
    BrightspaceCookie(String),
    /// Institutional host (e.g. `courses.maine.edu`). Synced for the same
    /// reason — a freshly-paired device shouldn't need re-entry.
    BrightspaceHost(String),
}

/// What changed in this batch of remote diffs. We collect a HashSet of
/// these in the synchronous Loro callback, then the async pump re-reads
/// each one from the doc and writes SQLite. Re-reading (rather than
/// trying to apply each delta surgically) makes the bridge robust to
/// any combination of nested ops in a single transaction.
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
enum Touched {
    /// Any change to the prefs root (single user_preferences row)
    Prefs,
    /// course_overlays.<org_unit_id>
    Course(String),
    /// assignment_overlays.<"course_id:bid">
    Assignment(String),
    /// grade_overlays.<"course_id:bid">
    Grade(String),
    /// content_item_overlays.<brightspace_id>
    ContentItem(String),
    /// notification_read.<external_id>
    Notification(String),
    /// synthetic_assignments.<uuid> (lives in `assignments` table)
    Synthetic(String),
}

/// What kind of row a pending overlay is waiting on. Mirrors the §10
/// overlay namespace so `drain_pending_overlay` can be called from
/// each Brightspace fetcher with a strongly-typed argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    Course,
    Assignment,
    Grade,
    ContentItem,
    Notification,
}

impl OverlayKind {
    fn as_str(self) -> &'static str {
        match self {
            OverlayKind::Course => "course",
            OverlayKind::Assignment => "assignment",
            OverlayKind::Grade => "grade",
            OverlayKind::ContentItem => "content_item",
            OverlayKind::Notification => "notification",
        }
    }
}

/// RAII guard for `Bridge::paused`. Set on construction, cleared on
/// drop — even if the caller panics or `?`-bails out of hydrate.
struct PauseGuard<'a> {
    flag: &'a AtomicBool,
}

impl<'a> PauseGuard<'a> {
    fn new(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::Release);
        Self { flag }
    }
}

impl Drop for PauseGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

/// Origin marker. Today the Loro `EventTriggerKind` filter inside
/// `Bridge::with_sql` is the actual mechanism that prevents echo storms,
/// so this enum doesn't gate any control flow. It stays in the public
/// surface as documentation of the contract: anything that ever needs
/// to know "did this SQLite write originate from a remote diff?" should
/// reach for this marker rather than re-inventing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Local,
    Remote,
}

// ---- Diff classification --------------------------------------------------

fn collect_touched(ev: &loro::event::DiffEvent) -> HashSet<Touched> {
    let mut out = HashSet::new();
    for cdiff in &ev.events {
        // Loro's `path` walks from the document root down to (and
        // including the index for) the target. `path[0].1` is the root
        // container's own name (`"course_overlays"`, `"prefs"`, …) —
        // NOT the first key inside the root. Real entity keys start at
        // `path[1].1`. Empty path is impossible: every diff has at
        // least one hop (the root-name hop).
        let Some((_, root_idx)) = cdiff.path.first() else { continue };
        let Index::Key(root_name) = root_idx else { continue };
        let root_name = root_name.as_ref();

        if cdiff.path.len() >= 2 {
            // Nested diff: the entity-level key lives at path[1].
            if let Some(Index::Key(key)) = cdiff.path.get(1).map(|(_, i)| i) {
                if let Some(t) = touched_for(root_name, key.as_ref()) {
                    out.insert(t);
                }
            }
            continue;
        }

        // path.len() == 1 → diff is the root container itself. Use the
        // root's MapDelta to learn which entries changed.
        if let Diff::Map(map_delta) = &cdiff.diff {
            // The prefs row maps to one SQLite row; any prefs-root diff
            // (display_name added, time_zone changed, semester_colors
            // sub-map appeared, …) lumps into a single re-write.
            if root_name == "prefs" {
                out.insert(Touched::Prefs);
                continue;
            }
            for (k, _) in &map_delta.updated {
                if let Some(t) = touched_for(root_name, k.as_ref()) {
                    out.insert(t);
                }
            }
        }
    }
    out
}

fn touched_for(root: &str, key: &str) -> Option<Touched> {
    Some(match root {
        "prefs" => Touched::Prefs,
        "course_overlays" => Touched::Course(key.to_string()),
        "assignment_overlays" => Touched::Assignment(key.to_string()),
        "grade_overlays" => Touched::Grade(key.to_string()),
        "content_item_overlays" => Touched::ContentItem(key.to_string()),
        "synthetic_assignments" => Touched::Synthetic(key.to_string()),
        "notification_read" => Touched::Notification(key.to_string()),
        _ => return None,
    })
}

// ---- SQLite mirror --------------------------------------------------------

async fn apply_touched(
    doc: &SyncDoc,
    pool: &SqlitePool,
    events: &dyn BridgeEventSink,
    set: HashSet<Touched>,
) {
    let mut prefs_changed = false;
    let mut courses_emitted: HashSet<String> = HashSet::new();
    let mut assignments_changed = false;
    let mut grades_changed = false;
    let mut notifications_changed = false;

    for t in set {
        match t {
            Touched::Prefs => {
                if let Err(e) = apply_prefs_to_sqlite(doc, pool).await {
                    warn!("apply_remote prefs failed: {e}");
                } else {
                    prefs_changed = true;
                }
            }
            Touched::Course(id) => {
                if let Err(e) = apply_course_to_sqlite(doc, pool, &id).await {
                    warn!("apply_remote course {id} failed: {e}");
                } else {
                    courses_emitted.insert(id);
                }
            }
            Touched::Assignment(key) => {
                if let Err(e) = apply_assignment_to_sqlite(doc, pool, &key).await {
                    warn!("apply_remote assignment {key} failed: {e}");
                } else {
                    assignments_changed = true;
                }
            }
            Touched::Grade(key) => {
                if let Err(e) = apply_grade_to_sqlite(doc, pool, &key).await {
                    warn!("apply_remote grade {key} failed: {e}");
                } else {
                    grades_changed = true;
                }
            }
            Touched::ContentItem(bid) => {
                if let Err(e) = apply_content_item_to_sqlite(doc, pool, &bid).await {
                    warn!("apply_remote content_item {bid} failed: {e}");
                }
            }
            Touched::Notification(eid) => {
                if let Err(e) = apply_notification_to_sqlite(doc, pool, &eid).await {
                    warn!("apply_remote notification {eid} failed: {e}");
                } else {
                    notifications_changed = true;
                }
            }
            Touched::Synthetic(uuid) => {
                if let Err(e) = apply_synthetic_to_sqlite(doc, pool, &uuid).await {
                    warn!("apply_remote synthetic {uuid} failed: {e}");
                } else {
                    assignments_changed = true;
                }
            }
        }
    }

    if prefs_changed {
        events.prefs_updated();
    }
    for c in courses_emitted {
        events.course_updated(&c);
    }
    if assignments_changed {
        events.assignments_updated();
    }
    if grades_changed {
        events.grades_updated();
    }
    if notifications_changed {
        events.notifications_updated();
    }
}

async fn apply_prefs_to_sqlite(doc: &SyncDoc, pool: &SqlitePool) -> Result<()> {
    write_prefs_to_sqlite(doc, pool).await
}

async fn write_prefs_to_sqlite(doc: &SyncDoc, pool: &SqlitePool) -> Result<()> {
    let display_name = doc.get_pref_display_name();
    let time_zone = doc.get_pref_time_zone();
    let historic_gpa = doc.get_pref_historic_gpa();
    let historic_units = doc.get_pref_historic_units();
    let default_semester = doc.get_pref_default_semester();
    let show_upcoming = doc.get_pref_show_upcoming_assignments().map(|b| b as i64);
    let show_course_list = doc.get_pref_show_course_list().map(|b| b as i64);
    let show_recent = doc.get_pref_show_recent_updates().map(|b| b as i64);
    let calendar_show_empty = doc.get_pref_calendar_show_empty_days().map(|b| b as i64);
    let brightspace_cookie = doc.get_pref_brightspace_cookie();
    let brightspace_host = doc.get_pref_brightspace_host();

    let semester_colors_json = serde_json::to_string(
        &doc.iter_pref_semester_colors()
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>(),
    )
    .unwrap_or_else(|_| "{}".to_string());

    let collapsed_topics_json = serde_json::to_string(
        &doc.iter_pref_collapsed_topics()
            .into_iter()
            .filter_map(|(k, v)| v.then_some(k))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());

    sqlx::query(
        "UPDATE user_preferences SET \
           display_name              = COALESCE(?, display_name), \
           time_zone                 = COALESCE(?, time_zone), \
           historic_gpa              = COALESCE(?, historic_gpa), \
           historic_units            = COALESCE(?, historic_units), \
           default_semester          = COALESCE(?, default_semester), \
           show_upcoming_assignments = COALESCE(?, show_upcoming_assignments), \
           show_course_list          = COALESCE(?, show_course_list), \
           show_recent_updates       = COALESCE(?, show_recent_updates), \
           calendar_show_empty_days  = COALESCE(?, calendar_show_empty_days), \
           brightspace_cookie        = COALESCE(?, brightspace_cookie), \
           brightspace_host          = COALESCE(?, brightspace_host), \
           semester_colors           = ?, \
           collapsed_topics          = ?, \
           updated_at                = CURRENT_TIMESTAMP",
    )
    .bind(display_name)
    .bind(time_zone)
    .bind(historic_gpa)
    .bind(historic_units)
    .bind(default_semester)
    .bind(show_upcoming)
    .bind(show_course_list)
    .bind(show_recent)
    .bind(calendar_show_empty)
    .bind(brightspace_cookie)
    .bind(brightspace_host)
    .bind(semester_colors_json)
    .bind(collapsed_topics_json)
    .execute(pool)
    .await?;
    Ok(())
}

async fn apply_course_to_sqlite(doc: &SyncDoc, pool: &SqlitePool, id: &str) -> Result<()> {
    let Some(o) = doc.get_course_overlay(id) else {
        // Entry vanished from the doc — nothing to mirror. (Course
        // overlay deletion isn't a feature today; this is defensive.)
        return Ok(());
    };
    apply_or_defer_course(pool, id, &o).await
}

async fn apply_or_defer_course(pool: &SqlitePool, id: &str, o: &CourseOverlay) -> Result<()> {
    let n = write_course_to_sqlite(pool, id, o).await?;
    if n == 0 {
        defer_overlay(pool, OverlayKind::Course, id, o).await?;
    }
    Ok(())
}

async fn write_course_to_sqlite(
    pool: &SqlitePool,
    id: &str,
    o: &CourseOverlay,
) -> Result<u64> {
    let res = sqlx::query(
        "UPDATE courses SET \
           is_pinned       = COALESCE(?, is_pinned), \
           custom_name     = ?, \
           custom_color    = ?, \
           custom_name     = ?, \
           custom_code     = ?, \
           units           = COALESCE(?, units), \
           target_grade    = COALESCE(?, target_grade), \
           sort_order      = COALESCE(?, sort_order), \
           end_of_week_day = COALESCE(?, end_of_week_day), \
           updated_at      = CURRENT_TIMESTAMP \
         WHERE org_unit_id = ?",
    )
    .bind(o.is_pinned.map(|b| b as i64))
    .bind(o.custom_name.as_deref()) // None → SQL NULL: clearing the override
    .bind(o.custom_color.as_deref()) // None → SQL NULL: clearing the override
    .bind(o.custom_name.as_deref()) // None → SQL NULL: clearing the override
    .bind(o.custom_code.as_deref()) // None → SQL NULL: clearing the override
    .bind(o.units)
    .bind(o.target_grade)
    .bind(o.sort_order)
    .bind(o.end_of_week_day)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Assignment overlays use `"<course_id>:<brightspace_id>"` as the key.
fn split_composite(key: &str) -> Option<(&str, &str)> {
    key.split_once(':')
}

async fn apply_assignment_to_sqlite(doc: &SyncDoc, pool: &SqlitePool, key: &str) -> Result<()> {
    let Some(o) = doc.get_assignment_overlay(key) else { return Ok(()) };
    apply_or_defer_assignment(pool, key, &o).await
}

async fn apply_or_defer_assignment(
    pool: &SqlitePool,
    key: &str,
    o: &AssignmentOverlay,
) -> Result<()> {
    let n = write_assignment_to_sqlite(pool, key, o).await?;
    if n == 0 {
        defer_overlay(pool, OverlayKind::Assignment, key, o).await?;
    }
    Ok(())
}

async fn write_assignment_to_sqlite(
    pool: &SqlitePool,
    key: &str,
    o: &AssignmentOverlay,
) -> Result<u64> {
    let Some((course_id, bid)) = split_composite(key) else { return Ok(0) };
    let res = sqlx::query(
        "UPDATE assignments SET \
           completed           = COALESCE(?, completed), \
           completed_at        = COALESCE(?, completed_at), \
           optional            = COALESCE(?, optional), \
           manually_edited     = COALESCE(?, manually_edited), \
           manually_edited_at  = COALESCE(?, manually_edited_at), \
           name                = COALESCE(?, name), \
           description         = COALESCE(?, description), \
           due_date            = COALESCE(?, due_date), \
           updated_at          = CURRENT_TIMESTAMP \
         WHERE course_id = ? AND brightspace_id = ?",
    )
    .bind(o.completed.map(|b| b as i64))
    .bind(o.completed_at)
    .bind(o.optional.map(|b| b as i64))
    .bind(o.manually_edited.map(|b| b as i64))
    .bind(o.manually_edited_at)
    .bind(o.name.as_deref())
    .bind(o.description.as_deref())
    .bind(o.due_date.as_deref())
    .bind(course_id)
    .bind(bid)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

async fn apply_grade_to_sqlite(doc: &SyncDoc, pool: &SqlitePool, key: &str) -> Result<()> {
    let Some(o) = doc.get_grade_overlay(key) else { return Ok(()) };
    apply_or_defer_grade(pool, key, &o).await
}

async fn apply_or_defer_grade(
    pool: &SqlitePool,
    key: &str,
    o: &GradeOverlay,
) -> Result<()> {
    let n = write_grade_to_sqlite(pool, key, o).await?;
    if n == 0 {
        defer_overlay(pool, OverlayKind::Grade, key, o).await?;
    }
    Ok(())
}

async fn write_grade_to_sqlite(
    pool: &SqlitePool,
    key: &str,
    o: &GradeOverlay,
) -> Result<u64> {
    let Some((course_id, bid)) = split_composite(key) else { return Ok(0) };
    let res = sqlx::query(
        "UPDATE grades SET \
           is_extra_credit          = COALESCE(?, is_extra_credit), \
           hidden                   = COALESCE(?, hidden), \
           manually_marked_ungraded = COALESCE(?, manually_marked_ungraded), \
           expected_score           = COALESCE(?, expected_score), \
           comments                 = COALESCE(?, comments), \
           updated_at               = CURRENT_TIMESTAMP \
         WHERE course_id = ? AND brightspace_id = ?",
    )
    .bind(o.is_extra_credit.map(|b| b as i64))
    .bind(o.hidden.map(|b| b as i64))
    .bind(o.manually_marked_ungraded.map(|b| b as i64))
    .bind(o.expected_score)
    .bind(o.comments.as_deref())
    .bind(course_id)
    .bind(bid)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

async fn apply_content_item_to_sqlite(
    doc: &SyncDoc,
    pool: &SqlitePool,
    brightspace_id: &str,
) -> Result<()> {
    let Some(o) = doc.get_content_item_overlay(brightspace_id) else { return Ok(()) };
    apply_or_defer_content_item(pool, brightspace_id, &o).await
}

async fn apply_or_defer_content_item(
    pool: &SqlitePool,
    brightspace_id: &str,
    o: &ContentItemOverlay,
) -> Result<()> {
    let Some(hidden) = o.is_hidden else { return Ok(()) };
    let res = sqlx::query(
        "UPDATE content_items SET is_hidden = ?, updated_at = CURRENT_TIMESTAMP \
         WHERE brightspace_id = ?",
    )
    .bind(hidden as i64)
    .bind(brightspace_id)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        defer_overlay(pool, OverlayKind::ContentItem, brightspace_id, o).await?;
    }
    Ok(())
}

async fn apply_notification_to_sqlite(
    doc: &SyncDoc,
    pool: &SqlitePool,
    external_id: &str,
) -> Result<()> {
    let Some(read) = doc.get_notification_read(external_id) else { return Ok(()) };
    apply_or_defer_notification(pool, external_id, read).await
}

async fn apply_or_defer_notification(
    pool: &SqlitePool,
    external_id: &str,
    read: bool,
) -> Result<()> {
    let res = sqlx::query(
        "UPDATE notifications SET is_read = ?, updated_at = CURRENT_TIMESTAMP \
         WHERE external_id = ?",
    )
    .bind(read as i64)
    .bind(external_id)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        let payload = serde_json::json!({ "is_read": read });
        defer_overlay_raw(
            pool,
            OverlayKind::Notification,
            external_id,
            &payload.to_string(),
        )
        .await?;
    }
    Ok(())
}

/// Synthetic assignments live in the regular `assignments` table with
/// `synthetic = 1` and `brightspace_id = <uuid>`. The Loro entry being
/// gone (None) means the synthetic was deleted on the peer; mirror that
/// with a DELETE here.
async fn apply_synthetic_to_sqlite(doc: &SyncDoc, pool: &SqlitePool, uuid: &str) -> Result<()> {
    match doc.get_synthetic_assignment(uuid) {
        Some(a) => {
            sqlx::query(
                "INSERT INTO assignments \
                   (course_id, brightspace_id, name, due_date, description, \
                    is_graded, completed, completed_at, synthetic, optional, \
                    created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, 0, ?, ?, 1, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
                 ON CONFLICT(course_id, brightspace_id) DO UPDATE SET \
                   name         = excluded.name, \
                   due_date     = excluded.due_date, \
                   description  = excluded.description, \
                   completed    = excluded.completed, \
                   completed_at = excluded.completed_at, \
                   optional     = excluded.optional, \
                   updated_at   = CURRENT_TIMESTAMP",
            )
            .bind(a.course_id.to_string())
            .bind(uuid)
            .bind(&a.name)
            .bind(a.due_date.as_deref())
            .bind(&a.description)
            .bind(a.completed as i64)
            .bind(a.completed_at)
            .bind(a.optional as i64)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query(
                "DELETE FROM assignments WHERE brightspace_id = ? AND synthetic = 1",
            )
            .bind(uuid)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

// ---- Pending overlay applications (T-011) --------------------------------
//
// When a Class-B overlay arrives for a row that hasn't been fetched
// from Brightspace yet (typically: hydrate_from_doc on a freshly
// paired device, or apply_remote racing the next sync), we park the
// overlay snapshot in `pending_overlay_apply`. Each Brightspace
// fetcher in `sync/*.rs` calls `drain_pending_overlay` after upserting
// a row; if a pending entry exists for `(kind, key)` we apply it on
// top and remove the pending row.

async fn defer_overlay<T: serde::Serialize>(
    pool: &SqlitePool,
    kind: OverlayKind,
    key: &str,
    payload: &T,
) -> Result<()> {
    let json = serde_json::to_string(payload)
        .map_err(|e| crate::error::AppError::Other(format!("defer_overlay serialize: {e}")))?;
    defer_overlay_raw(pool, kind, key, &json).await
}

async fn defer_overlay_raw(
    pool: &SqlitePool,
    kind: OverlayKind,
    key: &str,
    json: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO pending_overlay_apply (kind, key, payload, created_at) \
         VALUES (?, ?, ?, CURRENT_TIMESTAMP) \
         ON CONFLICT(kind, key) DO UPDATE SET \
            payload = excluded.payload, \
            created_at = CURRENT_TIMESTAMP",
    )
    .bind(kind.as_str())
    .bind(key)
    .bind(json)
    .execute(pool)
    .await?;
    Ok(())
}

/// Re-apply any pending overlay for `(kind, key)` and remove it.
///
/// Called by `sync/*.rs` Brightspace fetchers right after they upsert
/// a row, so any overlay that was waiting on that row gets applied on
/// top — preserving user intent (`is_pinned`, `custom_color`,
/// `manually_edited` …) across the initial-pair / Brightspace-sync
/// gap. No-ops when there's nothing pending.
///
/// If the JSON parse fails (e.g. the row was written by a future
/// schema version), we log + DELETE the row rather than spinning on
/// it forever — apply_remote will re-create it next time a peer
/// touches the entity.
pub async fn drain_pending_overlay(
    pool: &SqlitePool,
    kind: OverlayKind,
    key: &str,
) -> Result<()> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT payload FROM pending_overlay_apply WHERE kind = ? AND key = ?",
    )
    .bind(kind.as_str())
    .bind(key)
    .fetch_optional(pool)
    .await?;
    let Some((payload,)) = row else { return Ok(()) };

    let parse_err = |e: serde_json::Error| {
        warn!("drain_pending_overlay {} {}: bad payload: {e}", kind.as_str(), key);
    };

    match kind {
        OverlayKind::Course => match serde_json::from_str::<CourseOverlay>(&payload) {
            Ok(o) => {
                write_course_to_sqlite(pool, key, &o).await?;
            }
            Err(e) => parse_err(e),
        },
        OverlayKind::Assignment => match serde_json::from_str::<AssignmentOverlay>(&payload) {
            Ok(o) => {
                write_assignment_to_sqlite(pool, key, &o).await?;
            }
            Err(e) => parse_err(e),
        },
        OverlayKind::Grade => match serde_json::from_str::<GradeOverlay>(&payload) {
            Ok(o) => {
                write_grade_to_sqlite(pool, key, &o).await?;
            }
            Err(e) => parse_err(e),
        },
        OverlayKind::ContentItem => match serde_json::from_str::<ContentItemOverlay>(&payload) {
            Ok(o) => {
                if let Some(hidden) = o.is_hidden {
                    sqlx::query(
                        "UPDATE content_items SET is_hidden = ?, updated_at = CURRENT_TIMESTAMP \
                         WHERE brightspace_id = ?",
                    )
                    .bind(hidden as i64)
                    .bind(key)
                    .execute(pool)
                    .await?;
                }
            }
            Err(e) => parse_err(e),
        },
        OverlayKind::Notification => {
            // Stored as `{"is_read": bool}`.
            let parsed: std::result::Result<serde_json::Value, _> =
                serde_json::from_str(&payload);
            match parsed {
                Ok(v) => {
                    if let Some(read) = v.get("is_read").and_then(|x| x.as_bool()) {
                        sqlx::query(
                            "UPDATE notifications SET is_read = ?, updated_at = CURRENT_TIMESTAMP \
                             WHERE external_id = ?",
                        )
                        .bind(read as i64)
                        .bind(key)
                        .execute(pool)
                        .await?;
                    }
                }
                Err(e) => parse_err(e),
            }
        }
    }

    sqlx::query("DELETE FROM pending_overlay_apply WHERE kind = ? AND key = ?")
        .bind(kind.as_str())
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::doc::{CourseField, GradeField, SyntheticAssignment};
    use parking_lot::Mutex;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::time::Duration;
    use tokio::time::timeout;

    fn fresh() -> Arc<Bridge> {
        Bridge::new(Arc::new(SyncDoc::new()))
    }

    // ---- T-009 unit tests (apply_local) -----------------------------------

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
            CourseField::CustomName(Some("Intro to Psychology".into())),
            CourseField::CustomColor(Some("#abcdef".into())),
            CourseField::CustomName(Some("Calculus I".into())),
            CourseField::CustomCode(Some("MAT-101".into())),
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
        // CustomName is written twice above ("Intro to Psychology" then
        // "Calculus I"); last write wins, so the final overlay holds the latter.
        assert_eq!(got.custom_name.as_deref(), Some("Calculus I"));
        assert_eq!(got.custom_code.as_deref(), Some("MAT-101"));
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

    // ---- T-010 apply_remote tests -----------------------------------------

    #[derive(Default)]
    struct RecordingSink {
        course_updated: Mutex<Vec<String>>,
        assignments_updated: Mutex<u32>,
        grades_updated: Mutex<u32>,
        notifications_updated: Mutex<u32>,
        prefs_updated: Mutex<u32>,
    }

    impl BridgeEventSink for RecordingSink {
        fn course_updated(&self, course_id: &str) {
            self.course_updated.lock().push(course_id.to_string());
        }
        fn assignments_updated(&self) {
            *self.assignments_updated.lock() += 1;
        }
        fn grades_updated(&self) {
            *self.grades_updated.lock() += 1;
        }
        fn notifications_updated(&self) {
            *self.notifications_updated.lock() += 1;
        }
        fn prefs_updated(&self) {
            *self.prefs_updated.lock() += 1;
        }
    }

    /// Spin up an in-memory SQLite pool with the migrations applied.
    /// Each call gets its own pool — sqlite `:memory:` is per-connection,
    /// but `max_connections = 1` keeps everything on one shared db.
    async fn mem_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // Migration leaves user_preferences empty; insert the singleton row.
        sqlx::query("INSERT INTO user_preferences (api_port) VALUES (4567)")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    /// Wait up to 2s for `pred` (async closure) to return true. The
    /// apply_remote pump is async, so writes don't land synchronously
    /// after `import`. A sync-only predicate would deadlock here:
    /// running `block_on` inside `#[tokio::test]` blocks the same thread
    /// the sqlx future needs to make progress.
    async fn wait_for<F, Fut>(mut pred: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        timeout(Duration::from_secs(2), async {
            loop {
                if pred().await {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("apply_remote did not converge within 2s");
    }

    /// Apply A → import into B → assert B's SQLite mirror.
    async fn apply_into(b_doc: &SyncDoc, a_doc: &SyncDoc) {
        let bytes = a_doc.doc().export(loro::ExportMode::all_updates()).unwrap();
        b_doc.doc().import(&bytes).unwrap();
    }

    /// Acceptance test (ticket T-010): A pins a course; B's SQLite shows
    /// is_pinned=1 and the recording sink captured a `course_updated`.
    #[tokio::test]
    async fn apply_remote_course_pinned_propagates_to_sqlite_and_event() {
        let pool_b = mem_pool().await;
        let course_id = "12345";
        sqlx::query(
            "INSERT INTO courses (org_unit_id, name, semester, is_pinned) \
             VALUES (?, 'Intro to CRDTs', '2026-spring', 0)",
        )
        .bind(course_id)
        .execute(&pool_b)
        .await
        .unwrap();

        let sink_b: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let doc_a = Arc::new(SyncDoc::new());
        let doc_b = Arc::new(SyncDoc::new());
        let bridge_a = Bridge::new(doc_a.clone());
        let _bridge_b = Bridge::with_sql(
            doc_b.clone(),
            pool_b.clone(),
            sink_b.clone() as Arc<dyn BridgeEventSink>,
        );

        bridge_a
            .apply_local(LocalChange::Course {
                id: course_id.into(),
                field: CourseField::IsPinned(true),
            })
            .await
            .unwrap();

        apply_into(&doc_b, &doc_a).await;

        // Wait for B's pump to land the UPDATE.
        let pool_b_for_check = pool_b.clone();
        wait_for(|| {
            let pool = pool_b_for_check.clone();
            async move {
                let row: Option<(i64,)> =
                    sqlx::query_as("SELECT is_pinned FROM courses WHERE org_unit_id = ?")
                        .bind(course_id)
                        .fetch_optional(&pool)
                        .await
                        .ok()
                        .flatten();
                matches!(row, Some((1,)))
            }
        })
        .await;

        let emitted = sink_b.course_updated.lock().clone();
        assert_eq!(emitted, vec![course_id.to_string()]);
    }

    /// Confirms the echo-storm guard: a LOCAL apply on Bridge B does NOT
    /// trigger the apply_remote pump (which would re-write the SQLite row
    /// the local command already authored). We verify by counting events:
    /// the recording sink should stay quiet for purely-local activity.
    #[tokio::test]
    async fn apply_remote_skips_local_origin_diffs() {
        let pool = mem_pool().await;
        sqlx::query(
            "INSERT INTO courses (org_unit_id, name, is_pinned) \
             VALUES ('999', 'Local Course', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let doc = Arc::new(SyncDoc::new());
        let bridge = Bridge::with_sql(
            doc.clone(),
            pool.clone(),
            sink.clone() as Arc<dyn BridgeEventSink>,
        );

        bridge
            .apply_local(LocalChange::Course {
                id: "999".into(),
                field: CourseField::IsPinned(true),
            })
            .await
            .unwrap();

        // Give the pump a generous chance to (incorrectly) wake up.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let row: Option<(i64,)> =
            sqlx::query_as("SELECT is_pinned FROM courses WHERE org_unit_id = ?")
                .bind("999")
                .fetch_optional(&pool)
                .await
                .unwrap();
        // SQLite was NOT touched by apply_remote — the local INSERT-as-0
        // is still there, because the production wiring is: command writes
        // SQL, then calls apply_local. apply_local commits Loro, fires a
        // LOCAL diff event, which apply_remote skips by design.
        assert_eq!(row, Some((0,)));
        assert!(sink.course_updated.lock().is_empty());
    }

    /// Multiple roots changed in one peer batch → one event per affected
    /// kind; courses fan out per-id.
    #[tokio::test]
    async fn apply_remote_multi_kind_batch_emits_grouped_events() {
        let pool = mem_pool().await;
        sqlx::query(
            "INSERT INTO courses (org_unit_id, name, is_pinned) \
             VALUES ('A', 'Course A', 0), ('B', 'Course B', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO assignments (course_id, brightspace_id, name, completed) \
             VALUES ('A', 'a-1', 'HW 1', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notifications (external_id, notification_type, title, is_read) \
             VALUES ('ext-1', 'announcement', 'Welcome', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let doc_a = Arc::new(SyncDoc::new());
        let doc_b = Arc::new(SyncDoc::new());
        let bridge_a = Bridge::new(doc_a.clone());
        let _bridge_b = Bridge::with_sql(
            doc_b.clone(),
            pool.clone(),
            sink.clone() as Arc<dyn BridgeEventSink>,
        );

        bridge_a
            .apply_local(LocalChange::Course {
                id: "A".into(),
                field: CourseField::IsPinned(true),
            })
            .await
            .unwrap();
        bridge_a
            .apply_local(LocalChange::Course {
                id: "B".into(),
                field: CourseField::CustomColor(Some("#aabbcc".into())),
            })
            .await
            .unwrap();
        bridge_a
            .apply_local(LocalChange::Assignment {
                key: "A:a-1".into(),
                field: AssignmentField::Completed(true),
            })
            .await
            .unwrap();
        bridge_a
            .apply_local(LocalChange::NotificationRead {
                external_id: "ext-1".into(),
                read: true,
            })
            .await
            .unwrap();

        apply_into(&doc_b, &doc_a).await;

        let pool_for_check = pool.clone();
        wait_for(|| {
            let pool = pool_for_check.clone();
            async move {
                let a_pinned: Option<(i64,)> =
                    sqlx::query_as("SELECT is_pinned FROM courses WHERE org_unit_id = 'A'")
                        .fetch_optional(&pool)
                        .await
                        .ok()
                        .flatten();
                let b_color: Option<(Option<String>,)> = sqlx::query_as(
                    "SELECT custom_color FROM courses WHERE org_unit_id = 'B'",
                )
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
                let hw_done: Option<(i64,)> = sqlx::query_as(
                    "SELECT completed FROM assignments \
                     WHERE course_id = 'A' AND brightspace_id = 'a-1'",
                )
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
                let ext_read: Option<(i64,)> = sqlx::query_as(
                    "SELECT is_read FROM notifications WHERE external_id = 'ext-1'",
                )
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
                matches!(a_pinned, Some((1,)))
                    && matches!(&b_color, Some((Some(c),)) if c == "#aabbcc")
                    && matches!(hw_done, Some((1,)))
                    && matches!(ext_read, Some((1,)))
            }
        })
        .await;

        let mut courses = sink.course_updated.lock().clone();
        courses.sort();
        assert_eq!(courses, vec!["A".to_string(), "B".to_string()]);
        assert!(*sink.assignments_updated.lock() >= 1);
        assert!(*sink.notifications_updated.lock() >= 1);
    }

    #[tokio::test]
    async fn apply_remote_synthetic_assignment_insert_then_delete() {
        let pool = mem_pool().await;

        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let doc_a = Arc::new(SyncDoc::new());
        let doc_b = Arc::new(SyncDoc::new());
        let bridge_a = Bridge::new(doc_a.clone());
        let _bridge_b = Bridge::with_sql(
            doc_b.clone(),
            pool.clone(),
            sink.clone() as Arc<dyn BridgeEventSink>,
        );

        let uuid = "abcdef00-0000-4000-8000-000000000001";
        let synth = SyntheticAssignment {
            course_id: 42,
            name: "My deadline".into(),
            description: "Personal study time".into(),
            due_date: Some("2026-05-30".into()),
            optional: false,
            completed: false,
            completed_at: None,
            created_at: 1700000000,
        };

        bridge_a
            .apply_local(LocalChange::SyntheticAssignmentUpsert {
                uuid: uuid.into(),
                value: synth.clone(),
            })
            .await
            .unwrap();
        apply_into(&doc_b, &doc_a).await;

        let pool_check = pool.clone();
        wait_for(|| {
            let pool = pool_check.clone();
            async move {
                let row: Option<(String, i64)> = sqlx::query_as(
                    "SELECT course_id, synthetic FROM assignments WHERE brightspace_id = ?",
                )
                .bind(uuid)
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
                matches!(&row, Some((cid, 1)) if cid == "42")
            }
        })
        .await;

        bridge_a
            .apply_local(LocalChange::SyntheticAssignmentDelete { uuid: uuid.into() })
            .await
            .unwrap();
        apply_into(&doc_b, &doc_a).await;

        let pool_check = pool.clone();
        wait_for(|| {
            let pool = pool_check.clone();
            async move {
                let row: Option<(i64,)> =
                    sqlx::query_as("SELECT id FROM assignments WHERE brightspace_id = ?")
                        .bind(uuid)
                        .fetch_optional(&pool)
                        .await
                        .ok()
                        .flatten();
                row.is_none()
            }
        })
        .await;
    }

    #[tokio::test]
    async fn apply_remote_prefs_writes_all_columns_in_one_pass() {
        let pool = mem_pool().await;
        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let doc_a = Arc::new(SyncDoc::new());
        let doc_b = Arc::new(SyncDoc::new());
        let bridge_a = Bridge::new(doc_a.clone());
        let _bridge_b = Bridge::with_sql(
            doc_b.clone(),
            pool.clone(),
            sink.clone() as Arc<dyn BridgeEventSink>,
        );

        for change in [
            LocalChange::Pref(PrefField::DisplayName("Tucker".into())),
            LocalChange::Pref(PrefField::TimeZone("America/New_York".into())),
            LocalChange::Pref(PrefField::HistoricGpa(3.85)),
            LocalChange::Pref(PrefField::SemesterColor {
                semester: "2026-spring".into(),
                color: "#ff8800".into(),
            }),
            LocalChange::Pref(PrefField::CollapsedTopic {
                topic: "topic:42".into(),
                collapsed: true,
            }),
        ] {
            bridge_a.apply_local(change).await.unwrap();
        }

        apply_into(&doc_b, &doc_a).await;

        let pool_check = pool.clone();
        wait_for(|| {
            let pool = pool_check.clone();
            async move {
                let row: Option<(Option<String>, Option<String>, Option<f64>, String, String)> =
                    sqlx::query_as(
                        "SELECT display_name, time_zone, historic_gpa, semester_colors, collapsed_topics \
                         FROM user_preferences LIMIT 1",
                    )
                    .fetch_optional(&pool)
                    .await
                    .ok()
                    .flatten();
                match row {
                    Some((Some(name), Some(tz), Some(gpa), colors, topics))
                        if name == "Tucker"
                            && tz == "America/New_York"
                            && (gpa - 3.85).abs() < 1e-9 =>
                    {
                        colors.contains("2026-spring")
                            && colors.contains("#ff8800")
                            && topics.contains("topic:42")
                    }
                    _ => false,
                }
            }
        })
        .await;

        assert!(*sink.prefs_updated.lock() >= 1);
    }

    // ---- T-011 hydrate_from_doc + drain_pending_overlay tests -------------

    /// Acceptance test: hydrate_from_doc on an empty SQLite seeds the
    /// rows that ALREADY exist (prefs, courses with rows present), and
    /// parks composite-key entries with no underlying row into
    /// pending_overlay_apply for the Brightspace sync to drain later.
    #[tokio::test]
    async fn hydrate_from_doc_writes_existing_rows_and_defers_missing_ones() {
        let pool = mem_pool().await;
        // Course "12345" exists locally already (e.g. fetched during a
        // prior incomplete sync). Course "99999" does not — its overlay
        // must be deferred until Brightspace returns it.
        sqlx::query(
            "INSERT INTO courses (org_unit_id, name, is_pinned) VALUES ('12345', 'Existing', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let doc = Arc::new(SyncDoc::new());
        doc.set_pref_display_name("Tucker").unwrap();
        doc.set_course_overlay("12345", CourseField::IsPinned(true))
            .unwrap();
        doc.set_course_overlay("99999", CourseField::CustomColor(Some("#ff0000".into())))
            .unwrap();
        // assignment overlay for an assignment we haven't fetched yet
        doc.set_assignment_overlay(
            "777:abc",
            AssignmentField::Completed(true),
        )
        .unwrap();

        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let bridge = Bridge::with_sql(
            doc.clone(),
            pool.clone(),
            sink.clone() as Arc<dyn BridgeEventSink>,
        );

        bridge.hydrate_from_doc().await.unwrap();

        // Existing course got the overlay applied.
        let row: (i64,) =
            sqlx::query_as("SELECT is_pinned FROM courses WHERE org_unit_id = '12345'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, (1,));

        // Prefs row got the display_name written.
        let pref: (Option<String>,) =
            sqlx::query_as("SELECT display_name FROM user_preferences LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(pref.0.as_deref(), Some("Tucker"));

        // Missing-course overlay is parked.
        let pending: Vec<(String, String)> = sqlx::query_as(
            "SELECT kind, key FROM pending_overlay_apply ORDER BY kind, key",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            pending,
            vec![
                ("assignment".into(), "777:abc".into()),
                ("course".into(), "99999".into()),
            ],
        );
    }

    /// Drain re-applies a pending overlay AFTER the Brightspace fetcher
    /// inserts the underlying row. Mirrors the call sequence from
    /// `sync/courses.rs`.
    #[tokio::test]
    async fn drain_pending_overlay_applies_after_row_appears() {
        let pool = mem_pool().await;
        let doc = Arc::new(SyncDoc::new());

        // Pair-time: hydrate writes the overlay to pending (course doesn't exist yet).
        doc.set_course_overlay("99999", CourseField::CustomColor(Some("#abcdef".into())))
            .unwrap();
        doc.set_course_overlay("99999", CourseField::TargetGrade(Some(94.0)))
            .unwrap();

        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let bridge = Bridge::with_sql(
            doc.clone(),
            pool.clone(),
            sink.clone() as Arc<dyn BridgeEventSink>,
        );
        bridge.hydrate_from_doc().await.unwrap();

        // Brightspace sync now learns about course 99999 and inserts it.
        sqlx::query(
            "INSERT INTO courses (org_unit_id, name, is_pinned) VALUES ('99999', 'Late Course', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // …and immediately calls drain (this is what sync/courses.rs does).
        drain_pending_overlay(&pool, OverlayKind::Course, "99999")
            .await
            .unwrap();

        let row: (Option<String>, Option<f64>) = sqlx::query_as(
            "SELECT custom_color, target_grade FROM courses WHERE org_unit_id = '99999'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0.as_deref(), Some("#abcdef"));
        assert_eq!(row.1, Some(94.0));

        // Pending row removed.
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM pending_overlay_apply WHERE kind = 'course' AND key = '99999'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count.0, 0);
    }

    /// Drain on a key with nothing pending is a no-op.
    #[tokio::test]
    async fn drain_pending_overlay_is_noop_when_nothing_pending() {
        let pool = mem_pool().await;
        sqlx::query(
            "INSERT INTO courses (org_unit_id, name, is_pinned) VALUES ('123', 'Test', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Should succeed even though pending_overlay_apply has no row.
        drain_pending_overlay(&pool, OverlayKind::Course, "123")
            .await
            .unwrap();
    }

    /// apply_remote (via peer Update) on a key whose row doesn't exist
    /// yet must also defer. Verifies the deferred-on-zero-rows path
    /// fires from the live diff handler, not just from hydrate.
    #[tokio::test]
    async fn apply_remote_defers_when_target_row_missing() {
        let pool = mem_pool().await;
        // No `INSERT INTO courses` — the org_unit_id="lateppl" row doesn't
        // exist yet on this device.
        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let doc_a = Arc::new(SyncDoc::new());
        let doc_b = Arc::new(SyncDoc::new());
        let bridge_a = Bridge::new(doc_a.clone());
        let _bridge_b = Bridge::with_sql(
            doc_b.clone(),
            pool.clone(),
            sink.clone() as Arc<dyn BridgeEventSink>,
        );

        bridge_a
            .apply_local(LocalChange::Course {
                id: "lateppl".into(),
                field: CourseField::IsPinned(true),
            })
            .await
            .unwrap();
        apply_into(&doc_b, &doc_a).await;

        // Pump should have parked it.
        let pool_check = pool.clone();
        wait_for(|| {
            let pool = pool_check.clone();
            async move {
                let row: Option<(String,)> = sqlx::query_as(
                    "SELECT payload FROM pending_overlay_apply WHERE kind = 'course' AND key = 'lateppl'",
                )
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
                row.is_some()
            }
        })
        .await;
    }

    /// hydrate_from_doc must not race with concurrent Import diffs:
    /// while it's walking, the apply_remote pump is paused. After the
    /// walk completes, the pause is lifted automatically — verified
    /// by an Import diff for a different course landing in SQLite via
    /// the normal apply_remote path.
    #[tokio::test]
    async fn hydrate_pauses_apply_remote_then_resumes() {
        let pool = mem_pool().await;
        sqlx::query(
            "INSERT INTO courses (org_unit_id, name, is_pinned) VALUES \
             ('A', 'Course A', 0), ('B', 'Course B', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Two separate docs (mirrors the working apply_remote tests):
        //  - doc_b is the local doc with the bridge attached
        //  - doc_a is the peer that broadcasts the post-hydrate update
        let sink: Arc<RecordingSink> = Arc::new(RecordingSink::default());
        let doc_a = Arc::new(SyncDoc::new());
        let doc_b = Arc::new(SyncDoc::new());
        let bridge_a = Bridge::new(doc_a.clone());
        let bridge_b = Bridge::with_sql(
            doc_b.clone(),
            pool.clone(),
            sink.clone() as Arc<dyn BridgeEventSink>,
        );

        // Pre-seed doc_b's overlay and hydrate (sets courses.A.is_pinned=1).
        doc_b
            .set_course_overlay("A", CourseField::IsPinned(true))
            .unwrap();
        bridge_b.hydrate_from_doc().await.unwrap();

        let row: (i64,) =
            sqlx::query_as("SELECT is_pinned FROM courses WHERE org_unit_id = 'A'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, (1,), "hydrate must apply pre-existing overlay");

        // After hydrate returns, the pause guard has dropped — a fresh
        // peer-imported change for course B should flow through
        // apply_remote and emit a course_updated event.
        bridge_a
            .apply_local(LocalChange::Course {
                id: "B".into(),
                field: CourseField::CustomColor(Some("#123456".into())),
            })
            .await
            .unwrap();
        apply_into(&doc_b, &doc_a).await;

        let pool_check = pool.clone();
        wait_for(|| {
            let pool = pool_check.clone();
            async move {
                let row: Option<(Option<String>,)> = sqlx::query_as(
                    "SELECT custom_color FROM courses WHERE org_unit_id = 'B'",
                )
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
                matches!(&row, Some((Some(c),)) if c == "#123456")
            }
        })
        .await;

        let courses = sink.course_updated.lock().clone();
        assert!(courses.contains(&"B".to_string()));
    }
}
