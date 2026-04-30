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

/// Reconciles SQLite rows with the in-memory Loro doc.
/// Wired up by T-008.
pub struct Bridge {
    // T-008:
    //   pool:   sqlx::SqlitePool,
    //   doc:    Arc<SyncDoc>,
    //   events: EventBus,
    //   origin: parking_lot::RwLock<Origin>  (or per-task marker)
}

/// Local-origin change. One variant per Class-B SQLite write listed
/// in design.md §10. Filled in exhaustively by T-009 so the bridge's
/// match arms are compiler-checked.
#[derive(Debug, Clone)]
pub enum LocalChange {
    // expanded in T-009 — every column from the §10 field map
}

/// Remote-origin change derived from a Loro diff. Filled in by T-010.
#[derive(Debug, Clone)]
pub enum RemoteChange {
    // expanded in T-010 — mirrors LocalChange's coverage
}

/// Origin marker used to suppress echo storms (kickoff watch-out #1):
/// a Loro diff caused by a remote message must NOT bounce back into
/// `apply_local` and re-broadcast. The bridge sets `Origin::Remote`
/// on the relevant task before calling SQLite mutators, and
/// `apply_local` short-circuits when it sees that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Local,
    Remote,
}
