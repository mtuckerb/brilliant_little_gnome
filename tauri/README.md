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

## Layout

```
tauri/
├── src/                # React frontend
│   ├── api.ts          # invoke() wrappers
│   ├── types.ts        # mirrors src-tauri/src/models.rs
│   └── pages/
└── src-tauri/
    ├── migrations/     # sqlx-managed schema
    ├── src/
    │   ├── client/     # Brightspace HTTP client (stub)
    │   ├── commands/   # #[tauri::command] handlers
    │   ├── sync/       # orchestration + periodic loop
    │   ├── auth.rs     # JWT
    │   ├── db.rs       # pool + WAL setup
    │   ├── events.rs   # EventBus
    │   ├── error.rs    # AppError
    │   ├── models.rs   # data types
    │   ├── rest_api.rs # axum embedded server (stub)
    │   └── state.rs    # AppState
    └── Cargo.toml
```
