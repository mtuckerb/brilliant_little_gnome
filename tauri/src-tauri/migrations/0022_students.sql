-- A permanent directory, independent of each course's replaceable roster.
CREATE TABLE students (
  brightspace_user_id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  first_name TEXT,
  last_name TEXT,
  email TEXT,
  pronouns TEXT,
  imported_avatar TEXT,
  custom_avatar TEXT,
  avatar_overridden INTEGER NOT NULL DEFAULT 0,
  avatar_import_attempted INTEGER NOT NULL DEFAULT 0,
  first_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE student_courses (
  brightspace_user_id TEXT NOT NULL REFERENCES students(brightspace_user_id),
  course_id TEXT NOT NULL,
  course_name TEXT NOT NULL,
  semester TEXT,
  PRIMARY KEY (brightspace_user_id, course_id)
);
CREATE INDEX idx_student_courses_course ON student_courses(course_id);

CREATE TABLE student_roster_imports (
  course_id TEXT PRIMARY KEY,
  imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Seed from the newest saved name for each learner, including archived courses.
INSERT INTO students (brightspace_user_id, display_name, first_name, last_name, email, pronouns, first_seen_at, updated_at)
SELECT brightspace_user_id, display_name, first_name, last_name, email, pronouns, first_seen_at, updated_at
FROM (
  SELECT *, MIN(updated_at) OVER (PARTITION BY brightspace_user_id) AS first_seen_at,
    ROW_NUMBER() OVER (PARTITION BY brightspace_user_id ORDER BY updated_at DESC, course_id) AS position
  FROM course_roster
  WHERE (LOWER(COALESCE(role_name, '')) LIKE '%student%'
      OR LOWER(COALESCE(role_name, '')) LIKE '%learner%')
    AND LOWER(COALESCE(role_name, '')) NOT LIKE '%preview%'
)
WHERE position = 1;

INSERT INTO student_courses (brightspace_user_id, course_id, course_name, semester)
SELECT r.brightspace_user_id, r.course_id, COALESCE(c.name, r.course_id), c.semester
FROM course_roster r
LEFT JOIN courses c ON c.org_unit_id = r.course_id
WHERE r.brightspace_user_id IN (SELECT brightspace_user_id FROM students)
  AND (LOWER(COALESCE(r.role_name, '')) LIKE '%student%'
    OR LOWER(COALESCE(r.role_name, '')) LIKE '%learner%')
  AND LOWER(COALESCE(r.role_name, '')) NOT LIKE '%preview%';
