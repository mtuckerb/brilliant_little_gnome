# Brilliant

A lightweight, high-performance Brightspace companion that aggregates notifications, grades, and course content into a clean, unified dashboard. Designed specifically for University of Southern Maine students, Brilliant simplifies the complex Brightspace interface into a fast, desktop-priority experience.

## Features

- **Visual Dashboard**: Integrated course banners and normalized semestrial layouts.
- **Unified Notification Feed**: Newest items first, across all your courses.
- **Hero-Style Course Headers**: Clean, high-impact headers with integrated banner images for every course.
- **Local Asset Caching**: Persistent local storage for banners and resources to maintain speed and reliability.
- **Magic Login**: Connect to Brightspace instantly without manually hunting for cookies.
- **Advanced Analytics & GPA**: 
  - Real-time GPA calculation (USM-weighted).
  - "Max Potential" cumulative GPA tracking.
  - "Confidence Shields" (metric-driven data reliability scores).
- **Intelligent Synchronization**: Protecting historical data (archived courses/grades) even when instructor data on Brightspace thins.
- **Interactive Course View**: 
  - Collapsible instructions, feedback, and rubrics.
  - Consolidated Announcements & Notifications within the course sidebar.
- **Resource Export**: Download syllabus, module files, or entire course modules as ZIP archives.
- **Calendar Support**: Export assignment due dates to ICS/iCal format.

## Portability & Multi-platform
Brilliant is built to be portable. 
- **macOS (M-series)**: Fully vendored and optimized for Apple Silicon.
- **Windows**: Support for portable ruby distributions (x64) and packaged as a standalone Electron application (`nsis`).

## Setup

### 1. Prerequisites
- Ruby 2.6+
- SQLite3
- Chrome or Chromium (required for Magic Login)
- Bundler

### 2. Running the App
1. Install dependencies:
   ```bash
   bundle install
   ```
2. Start the server (Migrations will run automatically):
   ```bash
   ruby app.rb
   ```
3. Visit `http://localhost:4567` in your browser.

### 3. Authentication
Brilliant offers two ways to connect:

#### Option 1: Magic Login (Recommended)
1. On the setup screen, click **"Launch Magic Login"**.
2. A browser window will open. Log in to your school's Brightspace portal normally (MFA/SSO supported).
3. Once you reach the Brightspace home page, the window will close and Brilliant will automatically capture and securely store your session.

#### Option 2: Manual Cookie Entry
If you prefer not to use the automated tool:
1. Open DevTools (F12) in your browser on any Brightspace page.
2. Copy the `Cookie` request header value (or the full host URL).
3. Paste it into the "Manual Setup" field in Brilliant.

## Maintenance

### Refresh & Sync
The **Settings** menu now contains a dedicated maintenance section:
- **Reset & Sync Notifications**: Rebuilds your entire notification history and clears the API cache.
- **Re-Sync Courses**: Pulls fresh metadata (names, banners, codes) for all courses while intelligently protecting existing local data from degradation.

### Caching
Banners and assets are cached locally in the `public/banners` folder after the first successful authenticated fetch. This reduces network dependency and ensures your dashboard remains beautiful even in low-bandwidth situations.

## Design
Built with **Sinatra**, **ActiveRecord**, and **Bulma**. Managed as a high-performance sidecar for an **Electron** frontend.
