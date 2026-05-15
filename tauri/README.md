# brilliant-tauri

Tauri v2 port of the Sinatra/Ruby Brightspace companion app. Runs side-by-side
with the Ruby version until parity.

## Status

Scaffold + foundation:

- React + TypeScript frontend (Vite)
- Rust backend with sqlx/SQLite, JWT, axum (embedded REST API)
- Schema mirrored from `db/schema.rb` (initial migration `0001_init.sql`)
- Tauri commands for auth, prefs, courses, grades (with full computed summary),
  assignments, notifications, sync orchestration, REST API lifecycle
- Event bus replacing the SSE channel
- Background sync loop (every 15 min)

Stubbed (intentional — fills in next pass):

- `BrightspaceClient` HTTP fetchers and pagination
- Per-service sync (content modules, grades scrape, assignments, discussions, notifications)
- REST `/api/v1` route coverage (only `/health` is wired right now)
- Native cookie-capture window flow

## Getting started (NixOS)

```sh
cd tauri/
nix-shell                # provides webkitgtk, cairo, gtk3, glib, pkg-config, etc.
npm install
npm run tauri dev
```

## iOS / iPadOS

Use `npm run dev:ios` or `npm run dev:ios:remote` only for development; those commands intentionally load the WebView from the Vite dev server so hot module reload works.

Use `npm run ios:build` for a standalone iPhone/iPad artifact, or `npm run ios:open` to open the generated Xcode project for signing, archiving, and installing a release-style build. Release builds embed `dist/` assets via `frontendDist` and do not contact the Vite dev server on launch.

See [`../docs/ios.md`](../docs/ios.md) for the full dev-vs-release flow and offline validation checklist.

## Layout

```
tauri/
├── src/                # React frontend
│   ├── api.ts          # invoke() wrappers
│   ├── types.ts        # mirrors src-tauri/src/models.rs
│   ├── pages/
│   └── components/
│       └── SyncPanel.tsx   # Settings → Device Sync (T-015)
└── src-tauri/
    ├── migrations/     # sqlx-managed schema
    │   ├── 0001_init.sql
    │   ├── 0002_app_state.sql
    │   ├── 0003_sync_tracking.sql
    │   ├── 0004_calendar_prefs.sql
    │   ├── 0005_sync.sql              # p2p_enabled flag (Phase 2)
    │   ├── 0006_pending_overlay.sql   # initial-pair hydration deferral (T-011)
    │   └── 0007_pairing_nonces.sql    # one-shot QR nonces (T-012)
    ├── src/
    │   ├── client/     # Brightspace HTTP client
    │   ├── commands/   # #[tauri::command] handlers
    │   │   └── sync_p2p.rs # p2p_enable / pairing_qr / consume_pairing / …
    │   ├── sync/       # Brightspace sync orchestration
    │   ├── p2p/        # device-to-device sync (gated on `p2p` feature)
    │   │   ├── engine.rs        # orchestrator, lifecycle
    │   │   ├── transport.rs     # iroh endpoint + gossip
    │   │   ├── doc.rs           # Loro CRDT facade
    │   │   ├── bridge.rs        # SQLite ↔ Loro mirror
    │   │   ├── persistence.rs   # snapshot + WAL
    │   │   ├── pairing.rs       # QR payload + handshake
    │   │   └── secrets.rs       # OS keychain wrapper
    │   ├── auth.rs     # JWT
    │   ├── db.rs       # pool + WAL setup
    │   ├── events.rs   # EventBus
    │   ├── error.rs    # AppError
    │   ├── models.rs   # data types
    │   ├── rest_api.rs # axum embedded server
    │   └── state.rs    # AppState
    └── Cargo.toml
```

## Device-to-device sync (P2P)

Brilliant syncs the user's overlay edits — pinned courses, custom
colors, target grades, completion ticks, manual due-date overrides,
synthetic assignments — across the user's own devices. Brightspace
data itself (course names, descriptions, gradebook values) stays on
each device; only **your** edits travel.

The architecture, threat model, and field-level mapping are
documented in **[`docs/sync/design.md`](docs/sync/design.md)**. The
24-ticket roadmap that produced the current implementation is in
**[`docs/sync/tickets.md`](docs/sync/tickets.md)**.

### Cargo feature

```toml
[features]
default = ["custom-protocol", "p2p"]
p2p = ["dep:keyring"]
```

The `p2p` feature pulls in iroh (QUIC transport with NAT
hole-punching and relay fallback), iroh-gossip (pubsub), and Loro
(CRDT). Disable it with `--no-default-features --features
custom-protocol` for a smaller no-network build (CI, kiosks, dev
shells without internet).

### User-facing pairing flow

1. **Enable on device A**: open *Settings → Device Sync*, tick
   *Sync between my devices*. The app generates persistent secrets
   (stored in the OS keychain when available — see Secrets at rest
   below) and starts the engine.
2. **Show the QR**: click *Show pairing QR*. The QR encodes the
   device's iroh `EndpointId`, the shared `sync_doc_secret`, a
   one-shot 16-byte nonce, and an absolute 60-second expiry.
3. **Pair on device B**: click *Pair this device*. Mobile builds
   open the camera scanner; desktop falls back to a paste field.
   On submit, B persists the secret, dials A directly via the
   EndpointId, sends a `WireMsg::PairingRequest { nonce }`, A
   verifies the nonce hasn't been consumed and replies with
   `WireMsg::Snapshot`, B imports it, runs `hydrate_from_doc` to
   write the seed's overlays into B's SQLite, and the engine takes
   over for ongoing sync.
4. **Subsequent edits flow both ways automatically.** Each
   `apply_local` mutation broadcasts a Loro incremental update on
   the gossip topic; each peer's `apply_remote` translates the
   diff back into SQLite UPDATEs and emits the matching Tauri
   events (`course:updated`, `assignments:updated`, …) so the
   React UI re-fetches as if the change came from Brightspace.

### Rotation

If a device is lost, *Settings → Device Sync → Rotate secret*
generates a fresh `sync_doc_secret`, wipes the consumed-nonce
table, and restarts the engine on the new gossip topic. Old
devices remain on the old topic and never see new traffic — they
fall off silently. Re-pair every other device with a new QR.

### Secrets at rest

`iroh_node_secret` and `sync_doc_secret` live in the OS keychain
when available (macOS Keychain, Windows Credential Manager, Linux
kernel keyutils). On systems where the keychain isn't reachable
(headless Linux, sandboxed kiosks), they fall back to the
`app_state` SQLite table — same file as the rest of the
per-device flags. `sync_doc_secret` is wrapped in
`zeroize::Zeroizing<Vec<u8>>` once decoded so the raw 32 bytes
scrub from RAM on drop.

A pre-T-021 install with secrets in `app_state` migrates
transparently on the next start: the first
`load_or_init_secrets` call copies them into the keychain and
DELETEs the SQLite rows. After migration, `sqlite3
brilliant.sqlite3 .dump` no longer reveals raw secret material.

### Storage and limits

The engine writes a snapshot + WAL to
`app_data_dir()/sync/{doc.snapshot,doc.wal}`. Checkpoints fire on
the later of "every 60 seconds" or "200 WAL frames". Snapshot size
is surfaced live in the panel and a `p2p:warning` event fires if it
crosses 50 MB — at which point the user can rotate to start fresh.

