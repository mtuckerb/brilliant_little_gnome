// P2P device-to-device sync engine.
//
// Architecture: tauri/docs/sync/design.md §3
// Tickets:      tauri/docs/sync/tickets.md
//
// T-001 establishes only the crate dependencies and feature gate.
// T-003 fills in the submodules (engine, doc, transport, bridge,
// pairing, persistence) and wires `SyncEngine` into `AppState`.
