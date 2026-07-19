-- Device-local content cache. Lets a course's files/Tools/media be made
-- available offline; the viewer and the course export both read cache-first.
--
-- Only metadata lives here — the actual bytes are on the filesystem under
-- <app_data_dir>/content-cache/<course_id>/<topic_id>. This table is
-- deliberately NOT mirrored to Loro/P2P: cached content is large and
-- device-local, so it must never replicate across paired devices.

ALTER TABLE user_preferences ADD COLUMN cache_content INTEGER NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS content_cache (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  course_id TEXT NOT NULL,
  topic_id TEXT NOT NULL,
  rel_path TEXT NOT NULL,          -- path under the content-cache dir
  filename TEXT NOT NULL,          -- display / original filename (drives viewer routing)
  mime TEXT,
  byte_len INTEGER NOT NULL DEFAULT 0,
  item_kind TEXT NOT NULL,         -- 'file' | 'tool' | 'media'
  fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(course_id, topic_id)
);

CREATE INDEX IF NOT EXISTS idx_content_cache_course ON content_cache(course_id);
