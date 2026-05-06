# Claude Code kickoff prompt — P2P sync feature

Paste the prompt below into Claude Code (Opus 4.7) from inside the repo
root. It assumes you are on a checkout of `tauri-port` and have read/write
access to the working tree.

---

You are implementing a peer-to-peer sync feature for the Brilliant
Brightspace companion app (Tauri 2 / Rust / React). The full design and a
sequenced ticket list already exist in this repo:

- `tauri/docs/sync/design.md` — architecture, data model, threat model,
  field mapping
- `tauri/docs/sync/tickets.md` — 24 implementation tickets, dependency
  graph, acceptance criteria

**Read both documents before writing any code.** They are the contract for
this work. Treat them as authoritative; if you disagree with something,
flag it as a question rather than silently deviating.

## Working norms

- **Branch:** create `feat/p2p-sync` off `tauri-port`. Do not commit to
  `tauri-port` directly. All work for this feature lives on the feature
  branch until parity is reached.
- **Commits:** one commit per ticket, prefixed with the ticket ID, e.g.
  `T-001: add iroh, iroh-gossip, loro crates and p2p feature flag`.
- **Crate versions:** `tickets.md` lists target versions but says to verify
  current stable on crates.io at impl time. Do that — `iroh` 0.34 is the
  minimum; use whatever the latest stable is. Same for `loro`. Pin exact
  versions in `Cargo.toml` (no `^`); record the chosen versions in the
  commit message.
- **Tests:** every ticket has an acceptance criterion. Write the test as
  part of the ticket's commit when the criterion is testable. Tickets
  T-006, T-009, T-010, T-013, T-023 are integration-test heavy — these
  must have working tests before the ticket is marked complete.
- **No silent scope changes:** if you discover a ticket is wrong (e.g.
  `iroh-gossip`'s API has shifted), STOP and write a comment in the
  ticket explaining what changed, then ask before proceeding.
- **Style:** match the existing Rust style in `tauri/src-tauri/src/`. The
  repo uses `anyhow::Result` aliased through `crate::error::Result`,
  `parking_lot::RwLock` for sync state, `tokio` for async, and
  `tracing` for logging. Don't introduce new patterns without reason.

## Sequencing

Start with **T-001** and work through Phase 0 (T-001 → T-003) in order.
Phase 0 is pure scaffolding — at the end of it the project still builds
and runs unchanged but has empty modules and the new migration. That's
the right size for a first PR.

After Phase 0:
- Phase 1 (T-004, T-005) and Phase 2 (T-006, T-007) can run in parallel
  — they don't share files. If you have a habit of opening multiple work
  trees, use it here.
- Phase 3 (T-008 → T-011) blocks on both Phase 1 and Phase 2. This is
  the riskiest phase; budget extra time and write tests as you go.
- Phase 4 (T-012 → T-015) is mostly UI/Tauri command wiring once the
  engine works.
- Phase 5 (T-016, T-017) is mechanical wrapping of existing commands.
- Phase 6 (mobile) and Phase 7 (hardening) come last.

## Watch-outs from the spec author

These are the specific places things will probably go wrong:

1. **Bridge echo storms** (T-010). Remote-applied changes must NOT
   re-trigger `apply_local`. The simplest correct implementation is a
   thread-local or per-task `Origin` marker that the bridge checks before
   broadcasting. Get this right before T-016 lights up real traffic.
2. **Hydration race with Brightspace fetchers** (T-011). A freshly paired
   device receives overlay data for assignments it has not fetched yet.
   The `pending_overlay_apply` table (described in T-011) is the
   resolution — do not skip it.
3. **iroh API churn** (T-006). `iroh-gossip`'s public API has moved
   between minor versions; the sketch in the ticket may not compile
   against current crates. Read the iroh examples repo
   (`github.com/n0-computer/iroh-examples`) before writing T-006.
4. **iOS Local Network entitlement** (T-018). Without
   `NSLocalNetworkUsageDescription`, iroh on iOS falls back to relay-only
   silently. Test peer discovery on a physical iPhone, not a simulator.
5. **Loro `Text` vs string** (T-004). Use Loro `Text` ONLY for the four
   fields listed in T-004 — `prefs.display_name`,
   `assignment_overlays.<k>.{name,description}`,
   `synthetic_assignments.<u>.{name,description}`,
   `grade_overlays.<k>.comments`. Everything else is a plain value.
   Wrong choice here doubles per-update overhead for no benefit.

## Definition of done for the whole feature

- All 24 tickets closed.
- `cargo test --features p2p` passes, including the three-way convergence
  test (T-023).
- A physical iPhone build pairs to a Mac dev build and syncs assignment
  completion within 2s on Wi-Fi without using the iroh relay.
- Storage is bounded: with 1000 typical edits the snapshot stays under
  5 MB and the WAL under 500 KB at steady state.
- Settings → Sync UI lets a non-technical user pair a second device end
  to end without reading docs.

## Begin

Start by reading `tauri/docs/sync/design.md` end to end, then
`tauri/docs/sync/tickets.md`. Then plan Phase 0 (T-001, T-002, T-003) as
a single PR and execute. Report back with:

1. The exact crate versions you pinned.
2. A diff summary.
3. Anything in the spec that surprised you or that you had to adjust.

Use the TodoList tool to track ticket progress as you go.
