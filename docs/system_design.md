# Brilliant: System Design Document

## 1. Overview
Brilliant is a lightweight, high-performance companion app for the Brightspace LMS. It aggregates notifications, grades, and course content into a clean, unified dashboard, providing a more responsive and focused experience than the standard web portal.

## 2. Architecture
The application follows a **Hybrid Desktop Architecture**:
- **Host Environment**: Electron (Node.js) handles the window management, system integration, and native UI shell.
- **Backend (Sidecar)**: A Sinatra (Ruby) server running as a background process manages the business logic, database, and Brightspace API communication. The system follows a strict **Local-Database-First** model, where all UI components serve data from SQLite immediately, while background threads asynchronously reconcile with the LMS.
- **Database**: SQLite with ActiveRecord ORM for persistent local storage and synchronization.
- **Inter-process Communication (IPC)**: Node.js `spawn` manages the Ruby lifecycle, and the Renderer communicates with the Ruby sidecar via internal HTTP requests.

## 3. Core Components

### 3.1. Intelligent Synchronization Engine
The `BrilliantClient` is responsible for fetching data from the D2L Valence API.
- **Background Sync**: Operations run in non-blocking threads to keep the UI responsive.
- **High-Speed Persistence**: Uses `upsert_all` with composite unique indices (`course_id`, `brightspace_id`) to perform high-frequency batch writes. This minimizes I/O overhead and prevents primary key collisions during concurrent sync operations.
- **Thread Safety**: All background sync operations are wrapped in `ActiveRecord::Base.connection_pool.with_connection` to prevent database connection leakage and ensure stability in multi-threaded scenarios.
- **Data Protection**: Implements "Defensive Persistence." If an API call for an archived or restricted course returns "thin" metadata (e.g., missing descriptions or banner URLs), the local database preserves the existing robust data rather than overwriting it with nulls.
- **Sync Protection (Manual Overrides)**: Allows users to manually edit assignment names, descriptions, and due dates. A `manually_edited` flag in the database markers these records; when `true`, the sync engine skips updating these specific fields from the API. The system generates a notification after the sync process to list which items were protected, ensuring user transparency.
- **Concurrency**: SQLite is configured in WAL (Write-Ahead Logging) mode to support simultaneous reads and writes from the sync engine and the user interface.

### 3.2. Authentication (Magic Login) & Session Monitoring
- **Cookie Injection**: Captures authenticated session cookies via a dedicated Electron browser window.
- **Alert Suppression**: Injected JavaScript overrides `window.alert` and `window.confirm` during the login flow to prevent intrusive Brightspace error messages.
- **Degraded Mode**: The application monitors `403 Forbidden` responses. It distinguishes between resource-specific errors (e.g., archived courses) and global auth failures (e.g., expired session). Upon global failure, the client sets a `degraded_mode` flag.
- **Optimistic UI Polling**: The frontend layout polls the `/sync/status` endpoint. If `degraded_mode` is detected, a global notification banner is displayed to prompt for re-authentication via the Magic Login flow.

### 3.3. Asset Proxy & Caching
Because Brightspace assets (like course banners) often have strict CORS and Referer requirements, Brilliant uses a local proxy:
- **Proxy Route**: `/api/proxy/banner` fetches images using the authenticated Ruby session.
- **Disk Cache**: Images are hashed (SHA1) and stored in `public/banners/` to minimize redundant network requests and allow offline viewing.

### 3.4. External REST API & Multi-Device Access
Brilliant provides a robust REST API for external integrations:
- **Authentication**: Protected API and MCP requests accept the configured static key as a Bearer token, `X-API-Key`, or the `api_key` query parameter. They also accept short-lived JWTs issued by `/api/v1/token`.
- **Network Visibility**: The user can toggle between `localhost` (secure solo mode) and `0.0.0.0` (all interfaces) to allow remote access from other devices on the network.
- **Interactive documentation**: The embedded server exposes Swagger UI at `/docs` and its authoritative OpenAPI contract at `/openapi.yaml`.

### 3.5. Remote Server Logic
For users with multiple workstations, Brilliant supports a "Remote Server" mode:
- **Centralized Database**: When enabled, the application shifts its connection from the local SQLite file to a remote Brilliant instance (over the network).
- **Environment Targeting**: This is controlled via `remote_server_ip` and `remote_server_port` preferences, allowing a "Master/Member" architecture where one machine acts as the primary data sync node.

### 3.6. Intelligent Content Linkification
To ensure unified support for clickable resources across synthesized content:
- **URL Detection**: A robust regex-based helper (`fix_links`) scans course descriptions and task instructions for raw URLs (`https?://`).
- **Tag Shielding**: The engine uses a multi-pass approach where existing HTML tags are replaced with temporary placeholders before linkification. This prevents the creation of nested `<a>` tags or corruption of existing attributes.
- **Automatic Wrapping**: Detected URLs are wrapped in standard anchor tags with `target="_blank"`.

### 3.7. Markdown Persistence Strategy
- **Rich Text Extraction**: The sync engine automatically detects D2L Rich Text objects (JSON containing `Html` strings) and converts them to Markdown for persistent storage.
- **Background Sync**: Every course view triggers a background thread that fetches the latest LMS content, updates Markdown descriptions, and refreshes the cache without blocking the UI.
- **HTML-to-Markdown Pipeline**: A refined regex pipeline handles common LMS formatting (bold, links, lists, headers) to ensure consistent rendering across the app.

### 3.8. Persistent Discussion Model
Discussion data is fully integrated into the local-first database model:
- **Full Synchronization**: Forums, Topics, and individual Posts are synchronized to specific database tables (`discussion_forums`, `discussion_topics`, `discussion_posts`).
- **Metadata Enrichment**: To support advanced UI features like "Mine & Replied" sorting, the system calculates participation metadata (e.g., `UserIsAuthor`, `UserParticipated`) based on the local record set.
- **Background Reconciliation**: Every discussion view triggers a targeted background sync of the specific topic. If new posts are discovered, they are upserted into the database, and the UI is notified via the SSE event bus.

### 3.9. Live UI & Event-Driven Architecture (SSE)
To eliminate disruptive page reloads while maintaining a real-time feel, Brilliant uses a Server-Sent Events (SSE) pattern:
- **Event Bus**: A centralized `Brilliant::EventBus` publishes internal signals (e.g., `assignments_updated`, `grades_updated`, `notifications_synced`).
- **SSE Stream**: The `api_controller` exposes a persistent SSE endpoint (`/api/v1/events`) that transmits these signals to the frontend.
- **Delta-First Optimization**: The sync engine prioritizes Brightspace''s "Alerts" and "Feed" endpoints using a `since` timestamp offset. This eliminates the need to poll individual courses for updates, reducing API traffic by up to 90% during routine synchronization.
- **CustomEvent Dispatcher**: The global `layout.erb` listens to the SSE stream and dispatches `brilliant:*` CustomEvents to the browser window.
- **Targeted DOM Refresh**: Individual views (e.g., Dashboard, Assignments) subscribe to relevant events and trigger local `fetch()` calls to refresh specific UI containers dynamically. To prevent flickering during high-frequency updates, the UI implements a **2000ms debounce** on the EventSource listener.

## 3.10. Performance Optimization
- **SQLite WAL Mode**: Enabled by default to allow concurrent background writes and foreground reads.
- **Proactive Sync**: The application triggers course-wide syncs upon dashboard load and detailed item syncs upon specific page views, ensuring the local database is always trending toward completion.
- **Thin JSON Payloads**: API endpoints are optimized to serve only the data required for the current view, utilizing JSON serialization with `except` and `merge` to append calculated metadata.
- **Request Coalescing**: Implements per-path locking for in-flight API requests to prevent redundant simultaneous network calls for the same resource.
- **N+1 Prevention**: The notification sync loop leverages an in-memory `@course_model_cache` to avoid repeated database lookups for course metadata during batch processing of LMS alerts.
- **Database Indexing**: Critical tables (Notifications) are indexed on high-frequency query columns like `date`, `urgency`, and `is_read` to ensure sub-millisecond sorting and filtering.

## 4. Portability & Distribution
Brilliant is designed to be "Zero-Dependency" for the end user:
  - **macOS**: Uses a custom-provisioned Ruby 3.4 ARM64 runtime. The `RPATH` is dynamically patched during CI using `install_name_tool` to ensure absolute path independence (`@executable_path/../lib`).
  - **Windows**: Uses a portable RubyInstaller distribution embedded within the `bin/ruby_dist` directory.
- **CI/CD**: GitHub Actions automates the patching, codesigning (ad-hoc), and packaging into `.dmg` and `.exe` installers.

## 5. UI/UX Principles
- **Hero Headers**: Course detail pages feature dynamic banner backgrounds with metadata watermarking.
- **Unified Notifications**: Aggregates News, Content Updates, and Grades into a single, filterable stream.
- **GPA Analytics**: Local calculation of semester and cumulative GPA based on the University of Southern Maine (USM) scale, allowing for "what-if" grade tracking.

## 6. Security
- **Local-First**: Sensitive data remains on the user's machine within the SQLite database.
- **Authenticated Proxying**: All external requests are tunneled through the local client using the user's existing Brightspace session.
