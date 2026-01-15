# Brilliant

A lightweight, high-performance Brightspace companion that aggregates notifications, grades, and course content into a clean, unified dashboard.

## Features

- **Unified Notification Feed**: Newest items first, across all your courses.
- **Magic Login**: Connect to Brightspace instantly without manually hunting for cookies.
- **Analytics & GPA**: Real-time GPA calculation (USM-weighted) based on current semester grades and historical units.
- **Smart Filtering**: Filter by semester (e.g., "Spring 2026"), course, or urgency.
- **Read/Unread Status**: Mark items as read to focus on what's new. Syncs back to Brightspace (dismisses news items).
- **Interactive Course View**: Collapsible sections for instructions, feedback, and rubrics to keep your workspace clean.
- **Resource Download**: Download syllabus, module files, or entire course modules as ZIP archives.
- **Calendar Export**: Export assignment due dates to ICS/iCal format.

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
2. Copy the `Cookie` request header value.
3. Paste it into the "Manual Setup" field in Brilliant.

## Advanced Usage

### GPA & Analytics
Brilliant calculates your GPA by weighting current grades against course units. You can update your **Historic GPA** and **Historic Units** in the **Settings** menu to see your overall cumulative performance alongside your current semester stats.

### Persistence
The app remembers your preferences!
- **Collapse States**: Sections like assignment feedback or discussion instructions stay collapsed (or expanded) based on your last interaction.
- **Semester Filtering**: The dashboard will default to your most active semester.

### Database Management
If your notifications feel out of sync, use the **"Reset & Sync All"** button on the Notifications page. This clears the local cache/tables and triggers a fresh background sync.

## Design
Built with **Sinatra**, **ActiveRecord**, and **Bulma**. Designed for students who want to skip the heavy Brightspace UI and get straight to their data.
