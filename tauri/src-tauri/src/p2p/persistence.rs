// On-disk persistence for the Loro doc.
//
// Layout (design.md §9):
//   app_data_dir()/sync/
//   ├── doc.snapshot   most recent Loro snapshot (`ExportMode::Snapshot`)
//   ├── doc.wal        appended incremental updates since last snapshot
//   └── peers.json     last-seen NodeId cache (T-006 owns this file)
//
// WAL frame: `[u32 length, big-endian][bytes]`. No header, no checksum
// — Loro `import()` is idempotent and rejects malformed bytes, so a
// torn tail just gets logged-and-skipped on next load.
//
// Checkpoint atomicity: write snapshot to `doc.snapshot.tmp`, fsync,
// rename onto `doc.snapshot`, then truncate `doc.wal`. If we crash
// between rename and truncate, the WAL still holds updates that are
// already inside the snapshot — that's fine, Loro de-dupes on import.
//
// The "every 60s OR after 200 entries" scheduler is a SyncEngine
// concern (T-008): this module exposes `pending_wal_entries()` and
// `checkpoint()`, the engine drives them.

#![allow(dead_code)]

use crate::error::{AppError, Result};
use loro::{ExportMode, LoroDoc};
use parking_lot::Mutex;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::{AppHandle, Manager};
use tracing::warn;

const SNAPSHOT_FILE: &str = "doc.snapshot";
const WAL_FILE: &str = "doc.wal";
const SNAPSHOT_TMP: &str = "doc.snapshot.tmp";

/// Trigger a checkpoint after this many WAL appends, in addition to
/// the 60s timer the engine maintains. Tuned for "small enough that
/// startup load stays fast, large enough that we're not constantly
/// rewriting the snapshot." 200 ≈ a few minutes of active editing.
pub const CHECKPOINT_WAL_THRESHOLD: usize = 200;

pub struct SyncStore {
    dir: PathBuf,
    /// Append-only handle to `doc.wal`. Held open so each
    /// `append_update` is one syscall, not three (open/write/close).
    /// Behind a Mutex because `&self` methods need interior mutability
    /// and append_update is called from arbitrary tasks.
    wal: Mutex<File>,
    /// Count of frames written to the WAL since the last checkpoint.
    /// The engine's checkpoint scheduler reads this via `pending_wal_entries`.
    wal_entries: AtomicUsize,
}

impl SyncStore {
    /// Open the per-app sync directory. Creates it if missing.
    pub fn open(app: &AppHandle) -> Result<Self> {
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Other(format!("app_data_dir: {}", e)))?;
        Self::open_at(base.join("sync"))
    }

    /// Like `open`, but takes an explicit directory. Useful for tests.
    pub fn open_at(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir)?;
        let wal_path = dir.join(WAL_FILE);
        let wal = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&wal_path)?;
        // Best-effort count of existing frames so the checkpoint
        // scheduler kicks in even if we crashed mid-session.
        let initial_entries = count_wal_entries(&wal_path).unwrap_or_else(|e| {
            warn!("could not count existing wal entries: {e}");
            0
        });
        Ok(Self {
            dir,
            wal: Mutex::new(wal),
            wal_entries: AtomicUsize::new(initial_entries),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Hydrate a `LoroDoc` from disk: import the snapshot if any,
    /// then replay every well-framed WAL entry. Returns a fresh
    /// (empty) doc when neither file exists.
    pub fn load(&self) -> Result<LoroDoc> {
        let snapshot_path = self.dir.join(SNAPSHOT_FILE);
        let doc = if snapshot_path.exists() {
            let bytes = fs::read(&snapshot_path)?;
            LoroDoc::from_snapshot(&bytes)?
        } else {
            LoroDoc::new()
        };

        let wal_path = self.dir.join(WAL_FILE);
        if wal_path.exists() {
            let mut f = File::open(&wal_path)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            for frame in iter_wal_frames(&buf) {
                // Loro's `import` is idempotent; updates already represented
                // in the snapshot are silently no-ops.
                if let Err(e) = doc.import(frame) {
                    warn!("skipping malformed wal frame ({} bytes): {e}", frame.len());
                }
            }
        }
        Ok(doc)
    }

    /// Append one Loro update to the WAL. Bytes should come from
    /// `doc.export(ExportMode::updates(...))` (or whatever the engine
    /// captures from its local-update subscription).
    pub fn append_update(&self, bytes: &[u8]) -> Result<()> {
        let len = u32::try_from(bytes.len())
            .map_err(|_| AppError::Other(format!("wal frame too large: {} bytes", bytes.len())))?;
        let mut f = self.wal.lock();
        f.write_all(&len.to_be_bytes())?;
        f.write_all(bytes)?;
        // No fsync per write: design.md §9 explicitly trades durability
        // of the most recent few updates for write throughput. The
        // checkpoint path fsyncs the snapshot, so we only ever lose
        // sub-checkpoint tail on crash, which the peers will resend.
        self.wal_entries.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Number of WAL frames written since the last successful checkpoint.
    /// The SyncEngine compares this against `CHECKPOINT_WAL_THRESHOLD`.
    pub fn pending_wal_entries(&self) -> usize {
        self.wal_entries.load(Ordering::Relaxed)
    }

    /// Rewrite the snapshot atomically and truncate the WAL. Safe to
    /// call concurrently with `append_update`: the WAL lock serialises
    /// the truncate against in-flight appends.
    pub fn checkpoint(&self, doc: &LoroDoc) -> Result<()> {
        let snapshot_bytes = doc
            .export(ExportMode::Snapshot)
            .map_err(|e| AppError::Other(format!("loro export snapshot: {}", e)))?;
        let tmp_path = self.dir.join(SNAPSHOT_TMP);
        let final_path = self.dir.join(SNAPSHOT_FILE);

        // Atomic write: tmp + fsync + rename.
        {
            let mut tmp = File::create(&tmp_path)?;
            tmp.write_all(&snapshot_bytes)?;
            tmp.sync_all()?;
        }
        fs::rename(&tmp_path, &final_path)?;

        // Truncate WAL under the same lock that append_update takes,
        // so we can't lose an append that interleaved with this call.
        let mut wal = self.wal.lock();
        wal.set_len(0)?;
        wal.seek(SeekFrom::Start(0))?;
        self.wal_entries.store(0, Ordering::Relaxed);
        Ok(())
    }
}

/// Walk a WAL byte buffer, yielding each well-framed entry. Stops
/// silently at the first malformed/truncated record (a torn tail
/// from a crash mid-write — peers will resend whatever was lost).
fn iter_wal_frames(buf: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut cursor = 0usize;
    std::iter::from_fn(move || {
        if cursor + 4 > buf.len() {
            return None;
        }
        let len = u32::from_be_bytes([
            buf[cursor],
            buf[cursor + 1],
            buf[cursor + 2],
            buf[cursor + 3],
        ]) as usize;
        let start = cursor + 4;
        let end = start + len;
        if end > buf.len() {
            return None;
        }
        cursor = end;
        Some(&buf[start..end])
    })
}

fn count_wal_entries(path: &Path) -> Result<usize> {
    let bytes = fs::read(path)?;
    Ok(iter_wal_frames(&bytes).count())
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::doc::{AssignmentField, SyncDoc};
    use std::time::Instant;
    use tempfile::TempDir;

    fn fresh_store() -> (TempDir, SyncStore) {
        let tmp = TempDir::new().unwrap();
        let store = SyncStore::open_at(tmp.path().join("sync")).unwrap();
        (tmp, store)
    }

    #[test]
    fn load_returns_fresh_doc_when_no_files_present() {
        let (_tmp, store) = fresh_store();
        let doc = store.load().unwrap();
        // Empty doc has no version vector entries.
        assert_eq!(doc.state_vv().iter().count(), 0);
    }

    #[test]
    fn append_then_load_replays_updates() {
        let (_tmp, store) = fresh_store();

        // Build up some local state, capture each commit's bytes.
        let sync = SyncDoc::new();
        for i in 0..5 {
            let prev = sync.doc().state_vv();
            sync.set_pref_display_name(&format!("name-{i}")).unwrap();
            let update = sync
                .doc()
                .export(ExportMode::updates(&prev))
                .unwrap();
            store.append_update(&update).unwrap();
        }

        assert_eq!(store.pending_wal_entries(), 5);

        // Reopen from disk and verify the latest write replayed.
        let store2 = SyncStore::open_at(store.dir.clone()).unwrap();
        assert_eq!(store2.pending_wal_entries(), 5);
        let loaded = store2.load().unwrap();
        let replayed = SyncDoc::from_doc(loaded);
        assert_eq!(replayed.get_pref_display_name().as_deref(), Some("name-4"));
    }

    /// Ticket-mandated integration test: 500 mutations, drop, reopen,
    /// verify all present + verify checkpoint shrinks the WAL.
    #[test]
    fn five_hundred_mutations_round_trip_and_checkpoint_truncates_wal() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("sync");

        // Phase 1: open, mutate 500 times, drop.
        {
            let store = SyncStore::open_at(dir.clone()).unwrap();
            let sync = SyncDoc::new();
            for i in 0..500 {
                let prev = sync.doc().state_vv();
                let key = format!("100:{i}");
                sync.set_assignment_overlay(&key, AssignmentField::Completed(true))
                    .unwrap();
                let update = sync
                    .doc()
                    .export(ExportMode::updates(&prev))
                    .unwrap();
                store.append_update(&update).unwrap();
            }
            assert_eq!(store.pending_wal_entries(), 500);
        }

        let wal_before = fs::metadata(dir.join(WAL_FILE)).unwrap().len();
        assert!(wal_before > 0);

        // Phase 2: reopen, verify all 500 mutations replay.
        let store = SyncStore::open_at(dir.clone()).unwrap();
        let sync = SyncDoc::from_doc(store.load().unwrap());
        for i in 0..500 {
            let key = format!("100:{i}");
            let got = sync.get_assignment_overlay(&key).unwrap();
            assert_eq!(got.completed, Some(true), "missing key {key}");
        }
        assert_eq!(sync.iter_assignment_overlays().len(), 500);

        // Phase 3: checkpoint, then verify the WAL shrank to zero and
        // the snapshot now carries the state.
        store.checkpoint(&sync.doc()).unwrap();
        let wal_after = fs::metadata(dir.join(WAL_FILE)).unwrap().len();
        assert_eq!(wal_after, 0, "wal not truncated after checkpoint");
        assert_eq!(store.pending_wal_entries(), 0);
        assert!(
            fs::metadata(dir.join(SNAPSHOT_FILE)).unwrap().len() > 0,
            "snapshot not written"
        );

        // Phase 4: reopen one more time — load() should pick up the
        // snapshot with no WAL replay needed and still see all 500.
        let store2 = SyncStore::open_at(dir).unwrap();
        let sync2 = SyncDoc::from_doc(store2.load().unwrap());
        assert_eq!(sync2.iter_assignment_overlays().len(), 500);
    }

    #[test]
    fn malformed_wal_tail_is_skipped_not_fatal() {
        let (tmp, store) = fresh_store();
        let sync = SyncDoc::new();
        let prev = sync.doc().state_vv();
        sync.set_pref_display_name("ok").unwrap();
        let update = sync.doc().export(ExportMode::updates(&prev)).unwrap();
        store.append_update(&update).unwrap();

        // Simulate a torn tail: append a length header without payload bytes.
        {
            let mut wal = store.wal.lock();
            wal.write_all(&999u32.to_be_bytes()).unwrap();
            wal.write_all(b"oops").unwrap();
        }

        // load() should still recover the good frame.
        let store2 = SyncStore::open_at(tmp.path().join("sync")).unwrap();
        let replayed = SyncDoc::from_doc(store2.load().unwrap());
        assert_eq!(replayed.get_pref_display_name().as_deref(), Some("ok"));
    }

    /// Smoke timing for the 60s scheduler claim — 500 sequential
    /// appends should be well under 1s on any developer machine, so a
    /// 60s tick is the right knob, not the bottleneck.
    #[test]
    fn append_throughput_is_well_under_a_second() {
        let (_tmp, store) = fresh_store();
        let payload = vec![0u8; 256];
        let start = Instant::now();
        for _ in 0..500 {
            store.append_update(&payload).unwrap();
        }
        assert!(start.elapsed().as_secs() < 1, "wal appends are too slow");
    }
}
