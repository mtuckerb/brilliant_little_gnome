# Brilliant: System Design Document

## 1. Overview
Brilliant is a lightweight, high-performance companion app for the Brightspace LMS. It aggregates notifications, grades, and course content into a clean, unified dashboard, providing a more responsive and focused experience than the standard web portal.

## 2. Architecture
The application follows a **Hybrid Desktop Architecture**:
- **Host Environment**: Electron (Node.js) handles the window management, system integration, and native UI shell.
- **Backend (Sidecar)**: A Sinatra (Ruby) server running as a background process manages the business logic, database, and Brightspace API communication.
- **Database**: SQLite with ActiveRecord ORM for persistent local storage and synchronization.
- **Inter-process Communication (IPC)**: Node.js `spawn` manages the Ruby lifecycle, and the Renderer communicates with the Ruby sidecar via internal HTTP requests.

## 3. Core Components

### 3.1. Intelligent Synchronization Engine
The `BrilliantClient` is responsible for fetching data from the D2L Valence API.
- **Background Sync**: Operations run in non-blocking threads to keep the UI responsive.
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
- **JWT Authentication**: All API requests are authenticated via JSON Web Tokens (JWT) signed using a user-configurable `jwt_secret`. Tokens are generated internally and can be used by third-party clients (mobile apps, scripts).
- **Network Visibility**: The user can toggle between `localhost` (secure solo mode) and `0.0.0.0` (all interfaces) to allow remote access from other devices on the network.
- **Legacy API Key**: Maintain support for simple header-based authentication (`X-API-Key`) for legacy scripts.

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

### 3.8. Hybrid Discussion Content Strategy
Discussion data is handled through a tiered persistence model to balance synchronization speed with deep data availability:
- **Structural Persistence (DB)**: Discussion Forums and Topics are fully synchronized to the SQLite database. This ensures the course hierarchy is always available offline and supports fast navigation.
- **Content Caching (Stale-While-Revalidate)**: Individual Threads and Posts are managed via the `api_cache` system rather than strict ActiveRecord models. Content is fetched on-demand when a user views a topic and cached as raw JSON.
- **Metadata Enrichment**: To support advanced UI features like "Mine & Replied" sorting, the system dynamically enriches cached JSON payloads with participation metadata (e.g., `UserIsAuthor`, `UserParticipated`) calculated during retrieval.
- **Design Rationale**: This hybrid approach avoids the significant database overhead of mapping thousands of transient LMS posts to local records, ensuring the main sync process remains lightweight while providing rich, filterable content for active topics.

## 4. Portability & Distribution
Brilliant is designed to be "Zero-Dependency" for the end user:
- **Vendored Ruby**:
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
