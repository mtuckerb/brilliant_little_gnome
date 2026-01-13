# Brilliant

A lightweight, high-performance Brightspace companion that aggregates notifications, grades, and course content into a clean, unified dashboard.

## Features

- **Unified Notification Feed**: Newest items first, across all your courses.
- **Smart Filtering**: Filter by semester (e.g., "Spring 2026"), course, or urgency.
- **Read/Unread Status**: Mark items as read to focus on what's new. Syncs back to Brightspace (dismisses news items).
- **Dashboard Widget**: Quick view of unread updates.
- **Resource Download**: Download syllabus, module files, or entire course modules as ZIP archives.
- **Calendar Export**: Export assignment due dates to ICS/iCal format.

## Setup

### 1. Prerequisites
- Ruby 2.6+
- SQLite3
- Bundler

### 2. Configuration
The easiest way to authenticate is by using your browser's session cookies.

#### How to get `cookies.txt`:
1. Log in to your Brightspace instance in Chrome or Firefox.
2. Open the **Developer Tools** (F12) -> **Network** tab.
3. Refresh the page or click a link to a course.
4. Locate any request to your school's host (e.g., `courses.maine.edu`).
5. Look at the **Headers** -> **Request Headers**.
6. Find the `Cookie:` header. It will be a long string starting with something like `d2lt=...; d2l_referrer=...`.
7. Copy the **entire value** of the `Cookie:` header (everything after the word `Cookie: `).
8. Create a file named `cookies.txt` in the root of this project and paste the string inside.

Alternatively, if you have a developer token (Access Token), you can paste the raw token directly into `cookies.txt`.

### 3. Running the App
1. Install dependencies:
   ```bash
   bundle install
   ```
2. Initialize the database:
   ```bash
   bundle exec rake db:migrate
   ```
3. Set your host environment variable:
   ```bash
   export BS_HOST="your-school.brightspace.com"
   ```
4. Start the server:
   ```bash
   ruby app.rb
   ```
5. Visit `http://localhost:4567` in your browser.

## Database Management
If your notifications feel out of sync or you want a fresh start, use the **"Reset & Sync All"** button on the Notifications page. This will:
- Clear the local API cache.
- Wipe the local notifications table.
- Trigger a fresh background sync from the Brightspace API.

## Design
Built with **Sinatra**, **ActiveRecord**, and **Bulma**. Designed for students who want to skip the heavy Brightspace UI and get straight to their data.
