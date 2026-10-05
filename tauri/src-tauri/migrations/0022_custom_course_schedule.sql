-- User-owned meeting schedule overrides survive syllabus sync.
ALTER TABLE courses ADD COLUMN custom_meeting_days TEXT;
ALTER TABLE courses ADD COLUMN custom_meeting_time TEXT;
