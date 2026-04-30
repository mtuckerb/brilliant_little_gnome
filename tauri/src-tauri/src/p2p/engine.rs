// SyncEngine — top-level coordinator for P2P sync.
//
// Owns the Loro doc, the transport, the bridge, and the persistence
// layer. Lifecycle (start/shutdown), inbox handling, and broadcast
// fan-out are introduced by T-008.
//
// Today (T-003) this is a placeholder type so the rest of the module
// graph compiles and AppState can hold an `Option<Arc<SyncEngine>>`.

#![allow(dead_code)]

use crate::p2p::bridge::Bridge;
use crate::p2p::doc::SyncDoc;
use crate::p2p::persistence::SyncStore;
use crate::p2p::transport::Transport;
use std::sync::Arc;

pub struct SyncEngine {
    // T-008 will populate these and add a `start(state: Arc<AppState>)`
    // constructor that loads secrets, hydrates the SyncDoc from the
    // SyncStore, brings up the Transport, and spawns the bridge tasks.
    doc: Arc<SyncDoc>,
    store: SyncStore,
    transport: Transport,
    bridge: Arc<Bridge>,
    // node_id, peers map, last_apply_at — added when meaningful in T-008.
}
