-- Local-first copy of the Brightspace class list. This is deliberately
-- course-scoped and stores only the fields Brilliant shows in Overview.
-- A successful roster refresh replaces a course's rows atomically; failed or
-- empty responses leave the last-known class list intact.

CREATE TABLE IF NOT EXISTS course_roster (
  course_id TEXT NOT NULL,
  brightspace_user_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  first_name TEXT,
  last_name TEXT,
  email TEXT,
  role_name TEXT,
  role_id TEXT,
  pronouns TEXT,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (course_id, brightspace_user_id)
);

CREATE INDEX IF NOT EXISTS idx_course_roster_course
  ON course_roster(course_id);
