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
The `BrightspaceClient` is responsible for fetching data from the D2L Valence API.
- **Background Sync**: Operations run in non-blocking threads to keep the UI responsive.
- **Data Protection**: Implements "Defensive Persistence." If an API call for an archived or restricted course returns "thin" metadata (e.g., missing descriptions or banner URLs), the local database preserves the existing robust data rather than overwriting it with nulls.
- **Concurrency**: SQLite is configured in WAL (Write-Ahead Logging) mode to support simultaneous reads and writes from the sync engine and the user interface.

### 3.2. Authentication (Magic Login)
- **Cookie Injection**: Captures authenticated session cookies via a dedicated Electron browser window.
- **Alert Suppression**: Injected JavaScript overrides `window.alert` and `window.confirm` during the login flow to prevent intrusive Brightspace error messages from interrupting the automated cookie capture.

### 3.3. Asset Proxy & Caching
Because Brightspace assets (like course banners) often have strict CORS and Referer requirements, Brilliant uses a local proxy:
- **Proxy Route**: `/api/proxy/banner` fetches images using the authenticated Ruby session.
- **Disk Cache**: Images are hashed (SHA1) and stored in `public/banners/` to minimize redundant network requests and allow offline viewing.

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
