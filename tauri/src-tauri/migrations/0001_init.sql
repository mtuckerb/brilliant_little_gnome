-- Initial schema, mirrors db/schema.rb from the Ruby app.

CREATE TABLE IF NOT EXISTS user_preferences (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  display_name TEXT,
  time_zone TEXT,
  brightspace_host TEXT,
  brightspace_cookie TEXT,
  brightspace_uid TEXT,
  brightspace_user_id TEXT,
  api_enabled INTEGER NOT NULL DEFAULT 0,
  api_key TEXT,
  api_listen_all INTEGER NOT NULL DEFAULT 0,
  api_port INTEGER NOT NULL DEFAULT 4567,
  jwt_secret TEXT,
  semester_colors TEXT NOT NULL DEFAULT '{}',
  collapsed_topics TEXT NOT NULL DEFAULT '[]',
  historic_gpa REAL,
  historic_units REAL,
  default_semester TEXT,
  last_login_at TEXT,
  last_notification_sync_at TEXT,
  show_upcoming_assignments INTEGER NOT NULL DEFAULT 1,
  show_course_list INTEGER NOT NULL DEFAULT 1,
  show_recent_updates INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS courses (
  org_unit_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  code TEXT,
  semester TEXT,
  is_pinned INTEGER NOT NULL DEFAULT 0,
  custom_color TEXT,
  banner_url TEXT,
  units REAL,
  target_grade REAL DEFAULT 93.0,
  end_of_week_day INTEGER DEFAULT 4,
  overview_raw TEXT,
  status TEXT DEFAULT 'active',
  sort_order INTEGER DEFAULT 0,
  last_accessed_at TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS assignments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  course_id TEXT NOT NULL,
  brightspace_id TEXT NOT NULL,
  name TEXT NOT NULL,
  due_date TEXT,
  description TEXT,
  is_graded INTEGER NOT NULL DEFAULT 0,
  grade_item_id TEXT,
  assignment_type TEXT,
  completed INTEGER NOT NULL DEFAULT 0,
  completed_at TEXT,
  synthetic INTEGER NOT NULL DEFAULT 0,
  optional INTEGER NOT NULL DEFAULT 0,
  external_url TEXT,
  manually_edited INTEGER NOT NULL DEFAULT 0,
  manually_edited_at TEXT,
  attachments TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(course_id, brightspace_id)
);
CREATE INDEX IF NOT EXISTS idx_assignments_course ON assignments(course_id);
CREATE INDEX IF NOT EXISTS idx_assignments_due ON assignments(due_date);

CREATE TABLE IF NOT EXISTS grades (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  course_id TEXT NOT NULL,
  brightspace_id TEXT,
  name TEXT NOT NULL,
  displayed_grade TEXT,
  numerator REAL,
  denominator REAL,
  weight REAL,
  due_date TEXT,
  is_extra_credit INTEGER NOT NULL DEFAULT 0,
  hidden INTEGER NOT NULL DEFAULT 0,
  manually_marked_ungraded INTEGER NOT NULL DEFAULT 0,
  expected_score REAL,
  comments TEXT,
  grade_object_type TEXT,
  last_modified TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(course_id, brightspace_id)
);
CREATE INDEX IF NOT EXISTS idx_grades_course ON grades(course_id);

CREATE TABLE IF NOT EXISTS notifications (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  external_id TEXT NOT NULL UNIQUE,
  notification_type TEXT NOT NULL,
  title TEXT NOT NULL,
  body TEXT,
  date TEXT,
  course_id TEXT,
  course_name TEXT,
  semester TEXT,
  urgency INTEGER NOT NULL DEFAULT 0,
  is_personal INTEGER NOT NULL DEFAULT 0,
  is_read INTEGER NOT NULL DEFAULT 0,
  url TEXT,
  attachments TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_notifications_course ON notifications(course_id);
CREATE INDEX IF NOT EXISTS idx_notifications_date ON notifications(date DESC);

CREATE TABLE IF NOT EXISTS content_modules (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  course_id TEXT NOT NULL,
  brightspace_id TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT,
  sort_order INTEGER DEFAULT 0,
  parent_id TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(course_id, brightspace_id)
);
CREATE INDEX IF NOT EXISTS idx_modules_course ON content_modules(course_id);

CREATE TABLE IF NOT EXISTS content_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  module_id TEXT NOT NULL,
  brightspace_id TEXT NOT NULL,
  title TEXT NOT NULL,
  item_type TEXT,
  url TEXT,
  is_hidden INTEGER NOT NULL DEFAULT 0,
  sort_order INTEGER DEFAULT 0,
  attachments TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(module_id, brightspace_id)
);
CREATE INDEX IF NOT EXISTS idx_items_module ON content_items(module_id);

CREATE TABLE IF NOT EXISTS discussion_forums (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  brightspace_id TEXT NOT NULL,
  course_id TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(course_id, brightspace_id)
);

CREATE TABLE IF NOT EXISTS discussion_topics (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  brightspace_id TEXT NOT NULL,
  course_id TEXT NOT NULL,
  forum_id TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  sort_order INTEGER DEFAULT 0,
  thread_count INTEGER DEFAULT 0,
  post_count INTEGER DEFAULT 0,
  last_post_date TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(forum_id, brightspace_id)
);

CREATE TABLE IF NOT EXISTS discussion_posts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  brightspace_id TEXT NOT NULL,
  topic_id TEXT NOT NULL,
  thread_id TEXT,
  parent_post_id TEXT,
  subject TEXT,
  body TEXT,
  author_name TEXT,
  author_id TEXT,
  posted_at TEXT,
  is_instructor INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(topic_id, brightspace_id)
);

CREATE TABLE IF NOT EXISTS api_caches (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  path TEXT NOT NULL UNIQUE,
  data TEXT NOT NULL,
  is_archived INTEGER NOT NULL DEFAULT 0,
  user_id TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_caches_archived ON api_caches(is_archived);
