// On-disk persistence for the Loro doc.
//
// File layout (design.md §9):
//   app_data_dir()/sync/doc.snapshot
//   app_data_dir()/sync/doc.wal
//   app_data_dir()/sync/peers.json
//
// Real implementation lands in T-005. WAL framing is `[u32 length][bytes]`,
// snapshot is `loro::ExportMode::Snapshot` raw bytes. Checkpoint every
// 60s OR after 200 WAL entries.

#![allow(dead_code)]

use std::path::PathBuf;

/// Snapshot + WAL for the local Loro doc. Implemented by T-005.
pub struct SyncStore {
    dir: PathBuf,
}
