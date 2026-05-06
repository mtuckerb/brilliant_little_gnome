# Brilliant Sync — Implementation Tickets

**Repo path:** `tauri/docs/sync/tickets.md`
**Reads with:** `tauri/docs/sync/design.md`
**Branch:** create `feat/p2p-sync` off `tauri-port`

Tickets are sequenced. Earlier ones unblock later ones; deviating from order
will mean stubs everywhere. Each ticket lists exact file paths against
`tauri-port` HEAD, the crates it touches, and an acceptance criterion you
can run. Estimates assume one focused session each.

---

## Phase 0 — Scaffolding

### T-001 — Add CRDT + transport crates and feature-gate the module
**Touches:** `tauri/src-tauri/Cargo.toml`, `tauri/src-tauri/src/lib.rs`
**Crates:** `iroh`, `iroh-gossip`, `iroh-base`, `loro`, `qrcode`, `image`,
`blake3`, `postcard`, `zeroize`
**Steps:**
1. Add the crates from `design.md §11` to `[dependencies]`. Pin to current
   stable at the time of impl — verify `iroh` 0.34+ and `loro` 1.4+ on
   crates.io, don't take these numbers blindly.
2. Add `[features] p2p = []` and gate the new module behind it. (Default it
   on for development; we can flip later.)
3. Add a top-level `pub mod p2p;` in `lib.rs` behind `#[cfg(feature = "p2p")]`.
4. Verify `cargo build` and `cargo build --no-default-features` both pass.

**Acceptance:** `cargo check -p brilliant-tauri --features p2p` is clean.

---

### T-002 — Migration `0005_sync.sql`
**Touches:** `tauri/src-tauri/migrations/0005_sync.sql` (NEW)
**Steps:**
1. Create the migration:
   ```sql
   -- Persistent identity for P2P sync. iroh_node_secret is a 32-byte
   -- ed25519 secret (base64). sync_doc_secret is the shared 32-byte
   -- pairing secret (base64). p2p_enabled gates the engine entirely.
   ALTER TABLE user_preferences ADD COLUMN p2p_enabled INTEGER NOT NULL DEFAULT 0;

   INSERT INTO app_state (key, value) VALUES
     ('iroh_node_secret', NULL),
     ('sync_doc_secret', NULL),
     ('sync_doc_id', NULL)
   ON CONFLICT(key) DO NOTHING;
   ```
   *Note*: the `INSERT` won't actually work because `value TEXT` doesn't
   permit NULL inserts via that pattern; instead, leave the rows uninserted
   and have `p2p_enable` `INSERT OR REPLACE` them on first use. Adjust
   migration to just the `ALTER`.
2. Run `cargo run` once to ensure the migration applies cleanly on an
   existing dev DB.

**Acceptance:** `SELECT p2p_enabled FROM user_preferences;` returns `0` on
upgrade, schema otherwise unchanged.

---

### T-003 — Module skeleton `p2p/mod.rs`
**Touches:**
- `tauri/src-tauri/src/p2p/mod.rs` (NEW)
- `tauri/src-tauri/src/p2p/engine.rs` (NEW)
- `tauri/src-tauri/src/p2p/doc.rs` (NEW)
- `tauri/src-tauri/src/p2p/transport.rs` (NEW)
- `tauri/src-tauri/src/p2p/bridge.rs` (NEW)
- `tauri/src-tauri/src/p2p/pairing.rs` (NEW)
- `tauri/src-tauri/src/p2p/persistence.rs` (NEW)

**Steps:** create empty modules with `pub` traits and structs sketched
out matching `design.md §3`. No logic yet. Wire `p2p::SyncEngine` into
`AppState` as `pub sync: Option<Arc<SyncEngine>>`, default `None`.

**Acceptance:** `cargo check` passes; `AppState::initialize` returns with
`sync: None` and the existing app still boots and runs.

---

## Phase 1 — Loro doc

### T-004 — Define the Loro schema as typed accessors
**Touches:** `tauri/src-tauri/src/p2p/doc.rs`
**Steps:**
1. Implement `SyncDoc { inner: LoroDoc }` wrapping a `loro::LoroDoc`.
2. Add typed getters/setters mirroring `design.md §4` and §10:
   ```rust
   impl SyncDoc {
       pub fn set_pref_display_name(&self, value: &str) -> Result<()>;
       pub fn get_pref_display_name(&self) -> Option<String>;
       pub fn set_course_overlay(&self, id: &str, field: CourseField) -> Result<()>;
       pub fn iter_course_overlays(&self) -> impl Iterator<Item = (String, CourseOverlay)>;
       pub fn upsert_synthetic_assignment(&self, uuid: &str, a: &SyntheticAssignment) -> Result<()>;
       pub fn delete_synthetic_assignment(&self, uuid: &str) -> Result<()>;
       pub fn set_assignment_overlay(&self, key: &str, field: AssignmentField) -> Result<()>;
       pub fn set_grade_overlay(&self, key: &str, field: GradeField) -> Result<()>;
       pub fn set_notification_read(&self, external_id: &str, read: bool) -> Result<()>;
       // … one per syncable field in §10
   }
   ```
3. Strong-type the `*Field` enums so the bridge's match arms are exhaustive.
4. Use Loro `Text` only for: `prefs.display_name`,
   `assignment_overlays.<k>.{name,description}`,
   `synthetic_assignments.<u>.{name,description}`,
   `grade_overlays.<k>.comments`. Everything else is plain values.

**Acceptance:** unit test that exercises every setter+getter round-trips
correctly.

---

### T-005 — Persistence: snapshot + WAL
**Touches:** `tauri/src-tauri/src/p2p/persistence.rs`
**Steps:**
1. Implement:
   ```rust
   pub struct SyncStore { dir: PathBuf }
   impl SyncStore {
       pub fn open(app: &AppHandle) -> Result<Self>;            // creates app_data_dir/sync/
       pub fn load(&self) -> Result<LoroDoc>;                   // snapshot + WAL replay
       pub fn append_update(&self, bytes: &[u8]) -> Result<()>; // append-only
       pub fn checkpoint(&self, doc: &LoroDoc) -> Result<()>;   // rewrite snapshot, truncate WAL
   }
   ```
2. WAL format: `[u32 length][bytes]` records, no header.
3. Snapshot format: `loro::ExportMode::Snapshot` raw bytes.
4. On `load`, if neither file exists, return a fresh `LoroDoc`.
5. Background checkpoint task fires every 60s OR after 200 WAL entries.

**Acceptance:** integration test:
- create `SyncStore`, mutate doc 500 times, drop;
- reopen, verify all mutations present;
- verify checkpoint reduced WAL size.

---

## Phase 2 — Transport

### T-006 — Iroh endpoint and gossip topic
**Touches:** `tauri/src-tauri/src/p2p/transport.rs`
**Steps:**
1. Implement `Transport`:
   ```rust
   pub struct Transport {
       endpoint: iroh::Endpoint,
       gossip: iroh_gossip::net::Gossip,
       topic: iroh_gossip::proto::TopicId,
       outbox: tokio::sync::mpsc::Sender<Vec<u8>>,
       inbox: tokio::sync::broadcast::Sender<TransportEvent>,
   }
   pub enum TransportEvent {
       PeerConnected(NodeId),
       PeerDisconnected(NodeId),
       Message { from: NodeId, payload: Vec<u8> },
   }
   ```
2. `Transport::start(secret_key, sync_doc_secret) -> Result<Self>`:
   - Build endpoint with `secret_key` + `RelayMode::Default`.
   - Compute topic ID = `blake3(sync_doc_secret || b"brilliant-sync-v1")`.
   - Subscribe to topic, spawn background tasks that pump inbox/outbox.
3. `pub async fn broadcast(&self, payload: Vec<u8>)` enqueues on the outbox.
4. `pub fn subscribe(&self) -> broadcast::Receiver<TransportEvent>`.

**Acceptance:** integration test starts two Transports with the same secret
on localhost, subscribes both, broadcasts from one, asserts the other
receives it within 2s.

---

### T-007 — Message framing
**Touches:** `tauri/src-tauri/src/p2p/transport.rs`
**Steps:**
1. Define and serialise via `postcard`:
   ```rust
   #[derive(Serialize, Deserialize)]
   pub enum WireMsg {
       Update { bytes: Vec<u8> },           // Loro incremental update
       Snapshot { bytes: Vec<u8> },          // full Loro snapshot
       StateRequest { vv: Vec<u8> },         // state vector request
       StateResponse { vv: Vec<u8>, bytes: Vec<u8> }, // missing updates since vv
   }
   ```
2. All `Transport::broadcast` calls take a `WireMsg`, framing/unframing is
   internal.

**Acceptance:** unit tests for round-trip encode/decode of all variants.

---

## Phase 3 — Engine + bridge

### T-008 — `SyncEngine` orchestration
**Touches:** `tauri/src-tauri/src/p2p/engine.rs`
**Steps:**
1. ```rust
   pub struct SyncEngine {
       doc: Arc<SyncDoc>,
       store: SyncStore,
       transport: Transport,
       bridge: Arc<Bridge>,
       node_id: NodeId,
       peers: Arc<RwLock<HashMap<NodeId, PeerInfo>>>,
   }
   ```
2. `SyncEngine::start(state: Arc<AppState>) -> Result<Arc<Self>>`:
   - Load secrets from `app_state` (T-002). If missing, generate and persist.
   - Build `SyncDoc` via `SyncStore::load`.
   - Start `Transport`.
   - Start `Bridge` (T-009/T-010).
   - Spawn the inbox-handler task: on each `WireMsg`, dispatch to the
     appropriate `SyncDoc::import_*` and let the bridge do the rest.
   - On each local Loro change, capture the incremental update bytes,
     append to WAL, and `transport.broadcast(WireMsg::Update { bytes })`.
3. `SyncEngine::shutdown()`: checkpoint, drop transport.

**Acceptance:** start engine, mutate doc, shut down, restart, mutations
present and broadcast on second start (verify with two-engine integration
test).

---

### T-009 — Bridge: SQLite → Loro (apply_local)
**Touches:** `tauri/src-tauri/src/p2p/bridge.rs`
**Steps:**
1. Define a strongly typed `LocalChange` enum covering every Class-B SQLite
   write (see `design.md §10`).
2. `pub async fn apply_local(&self, change: LocalChange) -> Result<()>`:
   - Maps `change` onto one or more `SyncDoc` setter calls.
   - Captures incremental update bytes via `doc.export(ExportMode::updates_since(&last_vv))`.
   - Returns; the engine's broadcast task picks up the bytes.

**Acceptance:** unit test for every `LocalChange` variant: applies the
change and asserts the right Loro path holds the right value.

---

### T-010 — Bridge: Loro → SQLite (apply_remote)
**Touches:** `tauri/src-tauri/src/p2p/bridge.rs`
**Steps:**
1. Subscribe to the Loro doc with `doc.subscribe(|diff| ...)`.
2. For each diff event, walk the modified paths and emit one
   `RemoteChange::*` per affected field.
3. For each `RemoteChange`, run an `UPDATE` (or `INSERT OR REPLACE` for
   synthetic assignments) against SQLite.
4. After every batch of remote changes, emit the matching Tauri event:
   `course:updated` / `notifications:updated` / `prefs:updated` /
   `assignments:updated` / `grades:updated`.
5. Use a `bridge_origin` flag to avoid re-applying remote changes back into
   `apply_local`.
6. **Special case — synthetic assignments:** when `synthetic_assignments`
   adds a new entry, INSERT a new row in `assignments` with `synthetic = 1`
   and `brightspace_id = <uuid>`. When deleted, DELETE the row.

**Acceptance:** integration test:
- apply `LocalChange::CoursePinned` on engine A;
- assert engine B's SQLite shows `is_pinned = 1` for that course within 2s;
- assert engine B emits `course:updated`.

---

### T-011 — Bridge: initial-pair hydration
**Touches:** `tauri/src-tauri/src/p2p/bridge.rs`
**Steps:**
1. `pub async fn hydrate_from_doc(&self) -> Result<()>`:
   - Pause normal remote-change handling.
   - Walk every Loro path and translate to SQLite writes.
   - For composite-key entries that reference rows that don't exist yet
     (e.g. `assignment_overlays.<k>` for an assignment the new device hasn't
     fetched from Brightspace), record them in a new
     `pending_overlay_apply` table — apply when the corresponding
     Brightspace row arrives.
   - Resume.
2. New migration `0006_pending_overlay.sql`:
   ```sql
   CREATE TABLE IF NOT EXISTS pending_overlay_apply (
     kind TEXT NOT NULL,        -- 'assignment' | 'grade' | 'course' | 'content_item' | 'notification'
     key TEXT NOT NULL,
     payload TEXT NOT NULL,     -- JSON of the overlay fields
     created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
     PRIMARY KEY (kind, key)
   );
   ```
3. Modify each existing `sync/*.rs` Brightspace fetcher to drain matching
   `pending_overlay_apply` rows after writing a fresh row.

**Acceptance:** start engine A with full data, pair B with empty DB; after
hydration B has all overlays; B then runs Brightspace sync and the courses
hydrate with overlay flags intact.

---

## Phase 4 — Pairing UX

### T-012 — Pairing payload + QR generation
**Touches:** `tauri/src-tauri/src/p2p/pairing.rs`
**Steps:**
1. ```rust
   #[derive(Serialize, Deserialize, Clone)]
   pub struct PairingPayload {
       pub v: u8,                  // == 1
       pub node: String,           // NodeId in z32
       pub addrs: Vec<String>,     // direct addrs (best-effort hint)
       pub relay: Option<String>,  // relay url
       pub secret: String,         // sync_doc_secret hex
       pub nonce: String,          // 16 random bytes hex; one-shot
       pub exp: i64,               // unix ts; 60s in the future
   }
   ```
2. `pub fn generate_qr_png(payload: &PairingPayload) -> Result<Vec<u8>>`:
   serialise to JSON, base64, encode QR, render to 512×512 PNG bytes.
3. `pub fn parse_payload(json: &str) -> Result<PairingPayload>` validates
   `v == 1` and `exp > now` and `nonce` not already consumed.
4. New SQLite table `consumed_pairing_nonces (nonce TEXT PRIMARY KEY,
   consumed_at TEXT)` to enforce one-shot semantics.

**Acceptance:** unit tests for QR encode/decode round-trip; parse rejects
expired and replayed nonces.

---

### T-013 — Pairing handshake protocol
**Touches:** `tauri/src-tauri/src/p2p/pairing.rs`,
`tauri/src-tauri/src/p2p/transport.rs`
**Steps:**
1. Joining device on `consume_pairing(payload)`:
   - Persist `sync_doc_secret`, generate own `iroh_node_secret`.
   - Start engine.
   - Add the seed's NodeId to known peers, dial directly.
   - Send `WireMsg::StateRequest { vv: empty }`.
2. Seed device on receiving `StateRequest` from a fresh peer:
   - Verify nonce hasn't been consumed (record it now).
   - Reply with `WireMsg::Snapshot { bytes }`.
3. Joining device imports snapshot via `bridge.hydrate_from_doc()`.

**Acceptance:** integration test: spin up two engines on localhost, generate
pairing on A, consume on B, assert B's SQLite mirrors A's after 5s.

---

### T-014 — Tauri commands for pairing
**Touches:**
- `tauri/src-tauri/src/commands/sync_p2p.rs` (NEW)
- `tauri/src-tauri/src/commands/mod.rs`
- `tauri/src-tauri/src/lib.rs`
**Steps:**
1. Implement the seven commands from `design.md §8`.
2. Register them in `commands/mod.rs` and the `invoke_handler!` block in
   `lib.rs`.
3. Map errors to `AppError` (the existing `error.rs` pattern).

**Acceptance:** from a `cargo run` dev shell, invoke `p2p_enable` via
DevTools, see secrets persisted; invoke `p2p_pairing_qr`, get a PNG back.

---

### T-015 — React UI: Settings → Sync panel
**Touches:**
- `tauri/src/pages/Settings.tsx`
- `tauri/src/api.ts`
- `tauri/src/components/SyncPanel.tsx` (NEW)
- `tauri/src/components/QrScanner.tsx` (NEW; mobile-only)
**Steps:**
1. Add a "Device Sync" section in `Settings.tsx`.
2. Toggle: "Sync between my devices" → calls `p2p_enable` / `p2p_disable`.
3. When enabled:
   - Show this device's NodeId (truncated) and online peer count.
   - Button "Show pairing QR" → calls `p2p_pairing_qr`, shows QR for 60s
     with a countdown.
   - Button "Scan QR from another device" → opens `QrScanner`. On scan,
     calls `p2p_consume_pairing`.
4. List paired peers with last-seen timestamps. Status events
   (`p2p:peer_connected` etc.) update the list live.
5. Subscribe to `p2p:applied` events and show a toast ("Synced from
   <peer>") via the existing `ToastProvider`.

**Notes for mobile:** Tauri 2 Android needs `CAMERA` permission;
iOS needs `NSCameraUsageDescription` in `Info.plist`. Use
`@tauri-apps/plugin-barcode-scanner` (Tauri 2 official plugin) — far less
fragile than rolling your own MediaDevices code.

**Acceptance:** desktop dev: enable, generate QR, scan with phone build
running on simulator, observe peer connection toast and matching DB state.

---

## Phase 5 — Wire up writers

### T-016 — Wrap Class-B Tauri commands with `apply_local`
**Touches:** every command file that mutates Class-B fields:
- `commands/courses.rs`: `reorder_courses`, `update_course_color`,
  `update_course_units`, `update_course_target_grade`,
  `update_course_end_of_week`, `drop_course`
- `commands/grades.rs`: `toggle_grade_hidden`, `toggle_grade_ungraded`,
  `toggle_grade_extra_credit`, `set_expected_score`
- `commands/assignments.rs`: `toggle_assignment_complete`,
  `toggle_assignment_optional`, `update_assignment_due_date`,
  `create_synthetic_assignment`, `delete_assignment`
- `commands/notifications.rs`: `mark_notification_read`,
  `mark_all_notifications_read`
- `commands/prefs.rs`: `update_prefs`

**Pattern:**
```rust
// after the existing UPDATE
if let Some(sync) = state.sync.as_ref() {
    sync.bridge.apply_local(LocalChange::CoursePinned { id, pinned }).await?;
}
```

`if state.sync.is_some()` guard means engine-disabled mode is a no-op.

**Acceptance:** flip every command, run the existing test suite (if any),
manually flip every UI toggle and assert the corresponding Loro path
updates (debug command `p2p_dump_doc` is useful here — add as a temp tool).

---

### T-017 — Background sync (Brightspace) writes from B-class get re-broadcast
**Touches:** `tauri/src-tauri/src/sync/*.rs`
**Steps:** In each `sync/*.rs` fetcher, anywhere a Class-B field is touched
(notably the protect-manual-overrides logic), funnel through
`bridge.apply_local`. Most Brightspace writes are Class-A only and are
exempt — only flag the conflict-resolution paths.

**Acceptance:** trigger a Brightspace sync; assert that Class-A columns do
NOT appear in the Loro WAL; assert that protected-manual-override
reconciliations DO.

---

## Phase 6 — Mobile

### T-018 — iOS Local Network entitlement + camera
**Touches:** `tauri/src-tauri/gen/apple/...` (Xcode project under Tauri's
mobile gen dir)
**Steps:**
1. Add `NSLocalNetworkUsageDescription` and `NSBonjourServices` to
   `Info.plist` (Bonjour service type can be `_iroh._udp` if iroh's
   discovery uses it; otherwise omit).
2. Add `NSCameraUsageDescription` for the QR scanner.
3. Test on a physical device — simulator local network behaviour is
   incomplete.

**Acceptance:** physical iPhone build can discover and connect to the
Mac dev build over Wi-Fi without relay.

---

### T-019 — Android permissions and Doze handling
**Touches:** `tauri/src-tauri/gen/android/.../AndroidManifest.xml`
**Steps:**
1. Add `INTERNET`, `ACCESS_NETWORK_STATE`, `CAMERA`.
2. Listen to `onPause`/`onResume` from the Tauri Android bridge; on pause,
   `engine.checkpoint()`. On resume, `engine.reconnect()`.

**Acceptance:** pixel emulator build pairs and syncs; force-stopping the
app and reopening reconnects within 5s.

---

### T-020 — Lifecycle hooks on desktop
**Touches:** `tauri/src-tauri/src/lib.rs`
**Steps:** in the Tauri builder, add a `RunEvent::ExitRequested` handler
that calls `engine.shutdown().await`. Do the same on
`WindowEvent::CloseRequested`.

**Acceptance:** kill -9 ⇒ WAL replays correctly on restart;
graceful close ⇒ snapshot present, WAL minimal.

---

## Phase 7 — Hardening

### T-021 — Secrets at rest
**Touches:** `tauri/src-tauri/src/p2p/persistence.rs`,
`tauri/src-tauri/src/db.rs`
**Steps:**
1. Wrap `sync_doc_secret` in `zeroize::Zeroizing<Vec<u8>>` in memory.
2. Use the OS keychain (`tauri-plugin-keychain` or platform-specific
   `keyring` crate) for `iroh_node_secret` and `sync_doc_secret` on
   desktop. SQLite remains a fallback when keyring isn't available
   (Linux without secret-service).
3. Mobile: keychain on iOS, Keystore on Android.

**Acceptance:** dump of `brilliant.sqlite3` does NOT contain raw secret
material on systems with keychain available.

---

### T-022 — Compaction and size monitoring
**Touches:** `tauri/src-tauri/src/p2p/persistence.rs`
**Steps:** add a `p2p_storage_stats` Tauri command returning snapshot
size, WAL size, op count. Surface in Settings → Sync. If snapshot exceeds
50 MB, emit `p2p:warning` to the UI.

**Acceptance:** UI shows live storage numbers; mock-stuffed 50 MB doc
triggers warning.

---

### T-023 — End-to-end test: three-device convergence
**Touches:** `tauri/src-tauri/tests/p2p_three_way.rs` (NEW)
**Steps:** spin up three engines on localhost, pair two to one, generate
random concurrent edits across all three for 30s, assert all three docs
converge to identical state and SQLite tables match.

**Acceptance:** test passes 50 runs in CI without flake.

---

### T-024 — Documentation pass
**Touches:** `tauri/README.md`, `tauri/docs/sync/`
**Steps:** add a "Sync" section to the Tauri README pointing at
`docs/sync/design.md`; document the `p2p` cargo feature, the migrations,
and the user-facing pairing flow with a screenshot.

**Acceptance:** a colleague can follow the README and pair two devices
without asking questions.

---

## Sequencing summary

```
T-001 ─┬─ T-002 ─ T-003
       │
       ├─ T-004 ─ T-005 ────────┐
       │                        │
       ├─ T-006 ─ T-007 ──┐     │
       │                  │     │
       │                  ▼     ▼
       │                T-008 ─ T-009 ─ T-010 ─ T-011
       │                                            │
       │                                            ▼
       │                T-012 ─ T-013 ─ T-014 ─ T-015
       │                                            │
       │                                            ▼
       │                                          T-016 ─ T-017
       │                                                       │
       │                                                       ▼
       └─────────────────────── T-018, T-019, T-020 (parallel)
                                                                │
                                                                ▼
                                                         T-021 ─ T-022 ─ T-023 ─ T-024
```

## Estimate (very rough, focused-session count)

| Phase                          | Sessions |
|--------------------------------|----------|
| 0 Scaffolding (T-001 → T-003)  | 1        |
| 1 Loro doc (T-004 → T-005)     | 2        |
| 2 Transport (T-006 → T-007)    | 2        |
| 3 Engine + bridge (T-008 → T-011) | 4     |
| 4 Pairing UX (T-012 → T-015)   | 3        |
| 5 Wire writers (T-016 → T-017) | 1        |
| 6 Mobile (T-018 → T-020)       | 2        |
| 7 Hardening (T-021 → T-024)    | 3        |
| **Total**                      | **~18**  |

A focused weekend or two. The riskiest tickets are T-010 (bridge
correctness — get the suppression right or you'll get echo storms),
T-011 (hydration ordering with Brightspace fetchers), and T-018
(iOS Local Network is fiddly).
