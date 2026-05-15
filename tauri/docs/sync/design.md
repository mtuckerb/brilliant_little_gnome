# Brilliant Sync — Design Doc

**Repo path:** `tauri/docs/sync/design.md`
**Status:** Draft 1
**Targets:** Tauri 2.x desktop (mac/win/linux) + mobile (iOS/Android)
**Stack:** Iroh (transport, P2P, NAT traversal) + Loro (CRDT, conflict-free merge)

---

## 1. Goals

Brilliant currently keeps everything in a per-device SQLite file at
`app_data_dir()/brilliant.sqlite3`. Users (Tucker) own multiple devices —
laptop, iPad, phone — and want their **user-authored** state to flow between
them without configuration: scan a QR code on the second device, and from then
on every preference change, manual override, "completed" tick, archived course
flag, and synthetic assignment is reflected on every paired device within a
second or two.

What we are *not* trying to solve: multi-user collaboration. The "tenant" of a
sync group is one human with N devices. There is no notion of permissions
beyond "you have the pairing secret."

## 2. What syncs and what doesn't

The schema in `tauri/src-tauri/migrations/` mixes three kinds of data. Only the
second kind crosses the wire.

### Class A — Brightspace-sourced (NEVER sync)

Each device fetches these directly from Brightspace via the existing
`sync/*.rs` modules. They're effectively a cache of an external source of
truth, and re-fetching is cheap. Syncing them peer-to-peer would just mean
shipping the same bytes twice.

- `courses` columns: `name`, `code`, `semester`, `banner_url`, `status`,
  `last_accessed_at`, `overview_raw`, `created_at`, `updated_at`
- `assignments` columns: `name`, `due_date`, `description`,
  `is_graded`, `grade_item_id`, `assignment_type`, `external_url`,
  `attachments` — *unless* `manually_edited = 1` (then they're Class B)
- `grades` (everything except the user-overlay columns listed in B)
- `notifications` (everything except `is_read`)
- `content_modules`, `content_items` (everything except `is_hidden`)
- `discussion_forums`, `discussion_topics`, `discussion_posts`
- `api_caches`

### Class B — User-authored (SYNC via CRDT)

These are the "Brilliant value-add" — the manual overrides that distinguish
the app from Brightspace itself. Losing them on a device swap, or having them
out-of-sync between iPad and laptop, is the actual user pain point.

| Source                       | Syncable columns / values |
|------------------------------|----------------------------|
| `user_preferences`           | `display_name`, `time_zone`, `historic_gpa`, `historic_units`, `default_semester`, `semester_colors` (JSON), `collapsed_topics` (JSON), `show_upcoming_assignments`, `show_course_list`, `show_recent_updates`, `calendar_show_empty_days` |
| `courses` (overlay)          | `is_pinned`, `custom_color`, `units`, `target_grade`, `sort_order`, `end_of_week_day` |
| `assignments` (overlay)      | `completed`, `completed_at`, `optional`, `manually_edited`, `manually_edited_at`, manual `name` / `description` / `due_date` (only when `manually_edited = 1`), `synthetic` rows in their entirety |
| `grades` (overlay)           | `is_extra_credit`, `hidden`, `manually_marked_ungraded`, `expected_score`, `comments` |
| `content_items` (overlay)    | `is_hidden` |
| `notifications` (overlay)    | `is_read` |

### Class C — Per-device secrets and runtime (NEVER sync)

These are device-specific by definition. Including them would be a security
hole or a footgun.

- `user_preferences.brightspace_cookie` — per-device session
- `user_preferences.jwt_secret`, `api_key` — per-device REST API auth
- `user_preferences.api_enabled`, `api_listen_all`, `api_port` — per-device
  network config
- `user_preferences.last_login_at`, `last_notification_sync_at`,
  `courses.last_synced_at` — per-device sync state
- New: `app_state.iroh_node_secret`, `app_state.sync_doc_secret` — see §6

## 3. Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│  Brilliant (per device)                                              │
│                                                                      │
│   ┌──────────────────┐        ┌──────────────────┐                   │
│   │  React UI        │───────▶│  #[tauri::command]                   │
│   │  (existing)      │        │  handlers (existing)                 │
│   └──────────────────┘        └────────┬─────────┘                   │
│            ▲                           │                             │
│            │ Tauri events              ▼                             │
│   ┌────────┴─────────┐        ┌──────────────────┐                   │
│   │  EventBus        │◀───────│  SQLite (sqlx)   │                   │
│   │  (existing)      │        │  brilliant.sqlite3                   │
│   └──────────────────┘        └────────┬─────────┘                   │
│                                        │                             │
│                          ┌─────────────┴───────────────┐             │
│                          │                             │             │
│                          ▼                             ▼             │
│                  ┌───────────────┐           ┌──────────────────┐    │
│                  │  Brightspace  │           │  SyncEngine      │    │
│                  │  fetcher      │           │  (NEW)           │    │
│                  │  (existing)   │           │                  │    │
│                  └───────────────┘           │  ┌────────────┐  │    │
│                                              │  │  Loro doc  │  │    │
│                                              │  └─────┬──────┘  │    │
│                                              │        │         │    │
│                                              │  ┌─────┴──────┐  │    │
│                                              │  │  Iroh node │  │    │
│                                              │  │  + gossip  │  │    │
│                                              │  └─────┬──────┘  │    │
│                                              └────────┼─────────┘    │
└──────────────────────────────────────────────────────┬┴──────────────┘
                                                       │
                                                       ▼
                                              ┌─────────────────┐
                                              │  Other devices  │
                                              │  (peers via     │
                                              │   QUIC/relay)   │
                                              └─────────────────┘
```

The `SyncEngine` is a new module living at `tauri/src-tauri/src/p2p/`. It owns:

- A long-lived **Loro `LoroDoc`** holding the user-authored state (§4)
- An **Iroh `Endpoint`** + **`iroh-gossip` `Gossip`** instance (§5)
- A **bridge task** that translates Loro updates ↔ SQLite rows (§7)

It does *not* replace the existing `sync/` module — that one fetches from
Brightspace, this one syncs across the user's devices. They run in parallel.

## 4. CRDT schema (Loro doc layout)

A single Loro doc per user. Top-level is a `Map`:

```text
root: Map
├── prefs: Map<String, Value>
│     ├── display_name: Text
│     ├── time_zone: String
│     ├── historic_gpa: f64
│     ├── historic_units: f64
│     ├── default_semester: String
│     ├── semester_colors: Map<String, String>          # semester → hex
│     ├── collapsed_topics: Map<String, bool>           # topic key → collapsed
│     ├── show_upcoming_assignments: bool
│     ├── show_course_list: bool
│     ├── show_recent_updates: bool
│     └── calendar_show_empty_days: bool
│
├── course_overlays: Map<org_unit_id, Map>
│     └── { is_pinned, custom_color, units, target_grade,
│           sort_order, end_of_week_day }
│
├── assignment_overlays: Map<"<course_id>:<brightspace_id>", Map>
│     └── { completed, completed_at, optional,
│           manually_edited, manually_edited_at,
│           name?: Text, description?: Text, due_date?: String }
│
├── synthetic_assignments: Map<uuid, Map>               # entirely user-created
│     └── { course_id, name: Text, description: Text,
│           due_date, optional, completed, completed_at,
│           created_at }
│
├── grade_overlays: Map<"<course_id>:<brightspace_id>", Map>
│     └── { is_extra_credit, hidden, manually_marked_ungraded,
│           expected_score, comments: Text }
│
├── content_item_overlays: Map<brightspace_id, Map>
│     └── { is_hidden }
│
└── notification_read: Map<external_id, bool>
```

### Why this layout

- **Per-entity Maps** keep updates surgical: marking one assignment complete
  produces a few bytes, not a whole-doc diff.
- **`Text` CRDTs** for `description`, `comments`, `display_name`, and synthetic
  `name`/`description` mean concurrent edits from two devices merge cleanly
  character-by-character. (This is where "sync on newline in the editor" pays
  off — if Tucker is editing a synthetic assignment description on his iPad
  while autocorrect on his phone writes to the same field, it merges instead
  of clobbering.)
- **Composite keys** `"<course_id>:<brightspace_id>"` mirror the existing
  SQLite `UNIQUE(course_id, brightspace_id)` constraints and let the bridge
  do `(course_id, brightspace_id)` lookups without inventing new IDs.
- **`synthetic_assignments`** uses UUIDs because they have no Brightspace ID.
  The bridge writes them into `assignments` with `synthetic = 1` and the
  UUID stored in `brightspace_id` (consistent with how the Ruby app does it).

### What about deletes?

- Deleting a synthetic assignment: remove the entry from
  `synthetic_assignments`. Loro Map deletion is a CRDT op — no tombstone leak.
- Toggling `manually_edited` from 1 back to 0: leave the entry in
  `assignment_overlays` but clear the override fields. The bridge recognises
  "all override fields null" and reverts to the Brightspace values.

## 5. Transport (Iroh)

### Why iroh-gossip and not iroh-docs

`iroh-docs` ships its own per-key LWW CRDT, which would clash with Loro's
richer semantics. `iroh-gossip` is the right primitive: pubsub over QUIC with
NAT hole-punching and relay fallback baked in. Each Loro update becomes one
gossip message; receiving devices `import()` the bytes and the Loro merge
takes care of ordering.

### Topology

- Each device runs an `iroh::Endpoint` with a stable secret key persisted in
  `app_state.iroh_node_secret`.
- The "sync group" is one gossip topic. Topic ID is derived deterministically
  from the shared `sync_doc_secret` (§6) so only paired devices land on the
  same topic.
- Devices subscribe to the topic and broadcast their `NodeId` periodically;
  iroh handles dialing.
- Messages are framed as `(version, kind, payload)`:
  - `kind = 0`: Loro incremental update (`doc.export(ExportMode::updates)`)
  - `kind = 1`: Loro snapshot (sent on first connect, periodically, and on
    request)
  - `kind = 2`: Sync state vector exchange (so a freshly-paired device can
    request the parts it's missing)

### Mobile transport notes

- iroh works on iOS and Android. Both targets need to allowlist UDP for QUIC.
- iOS requires the **Local Network** entitlement for LAN peer discovery
  (`NSLocalNetworkUsageDescription` in `Info.plist`). Without it you're
  relay-only on iOS.
- Background sync on iOS is unreliable — assume the engine only runs while
  the app is foreground or in a small grace window. Snapshot on backgrounding;
  resync on foregrounding.
- Android is permissive but Doze mode will pause the gossip task — same
  pattern: snapshot before sleep, resync on wake.

## 6. Pairing (the QR handshake)

### Bootstrap (first device)

1. On first launch with sync enabled, generate:
   - `iroh_node_secret` (`iroh::SecretKey::generate()`) — persistent device
     identity.
   - `sync_doc_secret` (32 random bytes) — shared across all paired devices.
2. Persist both into `app_state` (new migration `0005_sync.sql`).
3. Maintain the durable peer roster in `paired_devices(id, public_key, last_seen_at, label)` (migration `0008_paired_devices.sql`). In Phase 2, `id` and `public_key` are both the iroh EndpointId/NodeId string because that value is the public-key-derived stable dial/display identity; no shared secrets or Brightspace credentials are stored there.
4. Topic ID = `blake3(sync_doc_secret || "brilliant-sync-v1")`.
5. The first device is now the implicit "seed"; its current Loro doc is the
   initial state.

### Add a device (QR pairing)

1. On the seed device: `Settings → Sync → Add device` shows a QR code
   containing:
   ```json
   {
     "v": 1,
     "node": "<seed_node_id_z32>",
     "addrs": ["<direct_addr_1>", "<direct_addr_2>"],
     "relay": "<relay_url>",
     "secret": "<sync_doc_secret_hex>"
   }
   ```
2. New device scans → parses → stores `sync_doc_secret`, computes topic ID,
   generates its own `iroh_node_secret`, and joins the gossip topic. It
   actively dials the seed's `NodeId` (from the QR) so it doesn't have to
   wait for discovery.
3. New device sends `kind=2` (state vector request).
4. Seed responds with `kind=1` (full snapshot up to its current frontier).
5. Bridge applies the snapshot → SQLite → emits events → UI refreshes.

### Rotation / revoke

- "Forget all paired devices" generates a new `sync_doc_secret`, rotates the
  topic, and broadcasts a final farewell. Old devices, still on the old
  topic, never see new traffic.
- There is no per-device revoke; possessing the secret = membership. If a
  device is lost, rotate.

### QR payload size

The above JSON is ~250 bytes when base64-packed. Fits comfortably in a
QR code at QR version 10, error correction M.

## 7. SQLite ↔ CRDT bridge

The bridge is the trickiest part. Two directions, both must be correct:

### Direction A: SQLite → Loro

Triggered by every Tauri command that mutates a Class-B field. Today these
are scattered across `commands/courses.rs`, `commands/grades.rs`,
`commands/assignments.rs`, `commands/notifications.rs`, `commands/prefs.rs`.

Two options:

1. **Wrap each command** to also call `engine.apply_local(...)`. Simple,
   surgical, slightly verbose.
2. **SQLite trigger + change feed**: write to a `crdt_outbox` table from
   triggers; engine consumes the outbox. More magical, harder to debug, but
   no command churn.

**Recommendation: option 1.** The list of Class-B commands is finite (~15)
and explicitly wrapping is auditable. Each mutating command does:

```rust
sqlx::query("UPDATE courses SET custom_color = ? WHERE org_unit_id = ?")
    .bind(&color).bind(&id).execute(&pool).await?;
state.sync.apply_local(LocalChange::Course {
    id: id.clone(),
    field: CourseField::CustomColor(color.clone()),
}).await?;
```

`apply_local` mutates the in-memory Loro doc, gets the resulting incremental
update bytes, persists them to disk (§9), and broadcasts on gossip.

### Direction B: Loro → SQLite

A long-running task subscribes to Loro doc changes
(`doc.subscribe(|diff| ...)`). On every diff:

1. Inspect the affected Loro paths (e.g.
   `course_overlays.<id>.is_pinned`).
2. Translate to SQL (e.g. `UPDATE courses SET is_pinned = ? WHERE
   org_unit_id = ?`).
3. **Suppress re-broadcast**: writes triggered by remote diffs must NOT
   re-enter `apply_local`. Use a per-task `bridge_origin: Origin::Remote`
   guard, or have `apply_local` skip if the change is a no-op.
4. Emit the matching Tauri event (`course:updated`, `prefs:updated`, etc.)
   so the React UI reactively re-fetches.

### Initial sync (paired-but-not-merged)

When a brand-new device pairs to a populated seed:

1. Bridge is paused.
2. Receive snapshot, hydrate the Loro doc.
3. Walk every Loro entry and `INSERT OR IGNORE` / `UPDATE` the matching
   SQLite row. For Class-A rows that don't exist yet (e.g. the new device
   has no `courses` row for a course yet), insert a stub with the overlay
   values, then let the next Brightspace sync fill in the rest.
4. Bridge resumes; subsequent diffs are routine.

### Schema concern: Class-A and Class-B in the same row

`courses.is_pinned` (Class B) and `courses.name` (Class A) live in the same
row. The bridge must only touch the B columns. Same for `assignments` (manual
override flags) and `grades`. This is fine but requires per-column
discipline. The mapping table is in §10.

## 8. Tauri command surface

New commands (all under `commands/sync_p2p.rs` to avoid colliding with the
existing Brightspace `commands/sync.rs`):

```rust
#[tauri::command]
pub async fn p2p_status(state: AppStateArg<'_>) -> Result<P2pStatus>;
// { enabled, node_id, paired_peers: [{node_id, last_seen_at}], last_apply_at }

#[tauri::command]
pub async fn p2p_enable(state: AppStateArg<'_>) -> Result<P2pStatus>;
// First call generates secrets + spawns engine. Idempotent.

#[tauri::command]
pub async fn p2p_disable(state: AppStateArg<'_>) -> Result<()>;

#[tauri::command]
pub async fn p2p_pairing_qr(state: AppStateArg<'_>) -> Result<PairingPayload>;
// Returns the JSON payload above (frontend renders the QR).

#[tauri::command]
pub async fn p2p_consume_pairing(
    state: AppStateArg<'_>,
    payload: PairingPayload,
) -> Result<P2pStatus>;
// Used by the joining device after scanning.

#[tauri::command]
pub async fn p2p_rotate(state: AppStateArg<'_>) -> Result<P2pStatus>;
// Generates new sync_doc_secret, drops old peers, requires re-pairing.
```

New events:

- `p2p:peer_connected { node_id }`
- `p2p:peer_disconnected { node_id }`
- `p2p:applied { kind: "course_overlay" | "assignment_overlay" | ..., id }`
- `p2p:error { message }`

The existing `course:updated`, `notifications:updated`, etc. events are
re-emitted by the bridge so the UI doesn't need new subscriptions for the
common path — it just sees its own data change.

## 9. Persistence

Loro doc state lives at:

```
app_data_dir()/sync/
├── doc.snapshot       # most recent Loro snapshot (binary)
├── doc.wal            # appended incremental updates since last snapshot
└── peers.json         # cache of last-seen NodeIds for fast reconnect
```

- On every local apply: append the update bytes to `doc.wal`.
- Every 60s OR after 200 wal entries: rewrite `doc.snapshot`, truncate `doc.wal`.
- On startup: `LoroDoc::from_snapshot(snapshot) + import(wal_entries)`.
- On graceful shutdown (`window-close`): force snapshot.

This is independent of the SQLite DB. SQLite remains the source of truth for
what the UI reads; the Loro files are the source of truth for what to
broadcast and what to merge with peers. They reconcile via the bridge.

## 10. Field mapping reference

| SQLite column                                   | Loro path                                                  |
|-------------------------------------------------|-------------------------------------------------------------|
| `user_preferences.display_name`                 | `prefs.display_name` (Text)                                |
| `user_preferences.time_zone`                    | `prefs.time_zone`                                           |
| `user_preferences.historic_gpa`                 | `prefs.historic_gpa`                                        |
| `user_preferences.historic_units`               | `prefs.historic_units`                                      |
| `user_preferences.default_semester`             | `prefs.default_semester`                                    |
| `user_preferences.semester_colors` (JSON)       | `prefs.semester_colors` (Map)                              |
| `user_preferences.collapsed_topics` (JSON)      | `prefs.collapsed_topics` (Map)                             |
| `user_preferences.show_upcoming_assignments`    | `prefs.show_upcoming_assignments`                           |
| `user_preferences.show_course_list`             | `prefs.show_course_list`                                    |
| `user_preferences.show_recent_updates`          | `prefs.show_recent_updates`                                 |
| `user_preferences.calendar_show_empty_days`     | `prefs.calendar_show_empty_days`                            |
| `courses.is_pinned`                             | `course_overlays.<id>.is_pinned`                            |
| `courses.custom_color`                          | `course_overlays.<id>.custom_color`                         |
| `courses.units`                                 | `course_overlays.<id>.units`                                |
| `courses.target_grade`                          | `course_overlays.<id>.target_grade`                         |
| `courses.sort_order`                            | `course_overlays.<id>.sort_order`                           |
| `courses.end_of_week_day`                       | `course_overlays.<id>.end_of_week_day`                      |
| `assignments.completed`                         | `assignment_overlays.<key>.completed`                       |
| `assignments.completed_at`                      | `assignment_overlays.<key>.completed_at`                    |
| `assignments.optional`                          | `assignment_overlays.<key>.optional`                        |
| `assignments.manually_edited`                   | `assignment_overlays.<key>.manually_edited`                 |
| `assignments.manually_edited_at`                | `assignment_overlays.<key>.manually_edited_at`              |
| `assignments.name` (when manually_edited)       | `assignment_overlays.<key>.name` (Text)                     |
| `assignments.description` (when manually_edited)| `assignment_overlays.<key>.description` (Text)              |
| `assignments.due_date` (when manually_edited)   | `assignment_overlays.<key>.due_date`                        |
| `assignments` row where `synthetic = 1`         | `synthetic_assignments.<uuid>`                              |
| `grades.is_extra_credit`                        | `grade_overlays.<key>.is_extra_credit`                      |
| `grades.hidden`                                 | `grade_overlays.<key>.hidden`                               |
| `grades.manually_marked_ungraded`               | `grade_overlays.<key>.manually_marked_ungraded`             |
| `grades.expected_score`                         | `grade_overlays.<key>.expected_score`                       |
| `grades.comments`                               | `grade_overlays.<key>.comments` (Text)                      |
| `content_items.is_hidden`                       | `content_item_overlays.<id>.is_hidden`                      |
| `notifications.is_read`                         | `notification_read.<external_id>`                           |

`<id>` = `org_unit_id` for courses, `brightspace_id` for content items,
`external_id` for notifications. `<key>` = `"<course_id>:<brightspace_id>"`.

## 11. Crate additions

```toml
# tauri/src-tauri/Cargo.toml additions

# Transport
iroh = "0.34"                     # check latest stable at impl time
iroh-gossip = "0.34"              # version-locked to iroh
iroh-base = "0.34"
quinn = "0.11"                    # transitive but pin for surety

# CRDT
loro = "1.4"                      # Loro 1.x API; verify latest

# Pairing
qrcode = "0.14"                   # QR encode for the seed device
image = "0.25"                    # render to PNG for the React UI

# Helpers
blake3 = "1.5"                    # topic ID derivation
postcard = "1"                    # compact framing for gossip messages
zeroize = "1"                     # wipe sync_doc_secret on rotation
```

(These add ~5–8 MB to the release binary, dominated by quinn and iroh's
dependency graph. Acceptable.)

## 12. Threats and non-goals

### Threat model

- **Adversary on the same network**: can see encrypted QUIC traffic,
  can't read it. iroh's transport is authenticated and encrypted by NodeId.
- **Adversary intercepts the QR**: gets full read/write access to the sync
  group. Mitigation: QR is shown for a bounded time (60s), and includes a
  `nonce` the seed verifies on first contact. Pairing fails after first use
  of a given nonce, so a screenshot replay one minute later fails.
- **Compromised relay**: iroh's relay only forwards encrypted bytes; can't
  read or tamper.
- **Lost device**: rotate `sync_doc_secret` (§6). Old devices fall off.

### Non-goals

- **Offline merging across many days**: Loro handles this fine, but realistic
  Brilliant usage will reconnect within hours, not weeks.
- **Audit log of who-changed-what**: out of scope. The CRDT carries vector
  clocks but we don't surface them.
- **Server-mediated sync without a peer online**: not in v1. If both your
  iPad and laptop are offline, your phone's edits queue locally and ship
  when one of them comes online. iroh's relay does NOT store-and-forward.
  v2 could add an optional self-hosted Iroh relay with an "always-on" peer.
- **Multi-user / family sharing**: explicit non-goal. One human, N devices.

## 13. Rollout

The flag `user_preferences.p2p_enabled` (new column in `0005_sync.sql`)
defaults to 0. The engine doesn't start until the user opts in via
`Settings → Sync`. This makes the whole feature dark-launchable behind a UI
toggle and means the default `cargo build` doesn't need network at startup.

## 14. Open questions

1. **Loro `Text` vs. plain string for `description`**: Text is correct for
   concurrent editing but doubles the per-field overhead. Defaulting to
   plain string and upgrading specific fields (assignment description,
   grade comments) to Text is a viable compromise. **Recommendation:** use
   Text only for the four fields that humans actually edit at length.

2. **What if Brightspace and the user-overlay disagree?** E.g. user marks
   `manually_edited = 1` with their own `name`, but a later Brightspace sync
   discovers the assignment's name changed upstream. Current behaviour
   (Ruby app): user's manual override wins. CRDT preserves this — the
   bridge writes Class-A data only when no overlay exists.

3. **Compaction**: long-lived Loro docs accumulate history. Loro supports
   GC (`doc.checkout` + new doc), but it loses concurrent-edit safety
   across the GC boundary. **Recommendation:** snapshot but don't GC in v1.
   Revisit if doc files grow above ~50 MB.

4. **iOS background**: we said snapshot-on-background, resync-on-foreground.
   But Tauri 2 mobile's lifecycle hooks are still maturing. Verify
   `RunEvent::Resumed` / `Suspended` fire reliably on iOS before assuming.

## 15. Why not [X]

- **Automerge**: works, but Loro's Rust API is more ergonomic and noticeably
  faster for the small-frequent-update workload Brilliant has.
- **Yjs/yrs**: also fine. Loro wins on benchmarks and has a friendlier
  builder API for typed Maps; Yjs leans on JS conventions (`Y.Map`,
  `Y.Array`) that translate awkwardly to Rust.
- **Just sync the SQLite WAL**: tempting (e.g. via `sqlite-sync` or
  Litestream-style replication) but doesn't merge: two devices editing the
  same row clobber each other. CRDT is the point.
- **Server-mediated (sync via the existing REST API)**: doesn't satisfy
  "configless" — needs an always-on Brilliant Server, which is the very
  thing the README calls out as optional. Iroh-relay is good enough.
- **Apple iCloud / CloudKit**: locks out non-Apple devices; the user has
  Android in the loop.
