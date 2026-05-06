// P2P device-to-device sync engine.
//
// Architecture: tauri/docs/sync/design.md §3
// Tickets:      tauri/docs/sync/tickets.md
//
// T-001 added the crate dependencies and feature gate.
// T-003 (this commit) lays down empty submodules with `pub` types
// sketched per design §3. Subsequent tickets fill in real bodies:
//
//   T-004 → doc.rs        (typed Loro accessors)
//   T-005 → persistence.rs (snapshot + WAL)
//   T-006 → transport.rs  (iroh endpoint + gossip topic)
//   T-007 → transport.rs  (WireMsg framing)
//   T-008 → engine.rs     (orchestration, start/shutdown)
//   T-009 → bridge.rs     (apply_local: SQLite → Loro)
//   T-010 → bridge.rs     (apply_remote: Loro → SQLite + echo guard)
//   T-011 → bridge.rs     (hydrate_from_doc: initial-pair drain)
//   T-012/T-013 → pairing.rs (QR payload + handshake)

pub mod bridge;
pub mod doc;
pub mod engine;
pub mod pairing;
pub mod persistence;
pub mod secrets;
pub mod transport;

pub use engine::SyncEngine;
