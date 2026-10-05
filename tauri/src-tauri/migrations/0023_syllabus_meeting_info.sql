-- Share the extracted room and schedule, keeping manual overrides separate.
ALTER TABLE courses ADD COLUMN syllabus_meeting_info TEXT;
-- Per-device pending bits: room=1, days=2, time=4, extracted values=8.
ALTER TABLE courses ADD COLUMN meeting_sync_pending INTEGER NOT NULL DEFAULT 0;
