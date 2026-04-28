-- Per-course sync tracking, used by Phase A "skip if recently synced" probe
-- in sync/mod.rs. last_notification_sync_at already lives on user_preferences
-- and is reused for the alerts/feed ?since= window.

ALTER TABLE courses ADD COLUMN last_synced_at TEXT;
