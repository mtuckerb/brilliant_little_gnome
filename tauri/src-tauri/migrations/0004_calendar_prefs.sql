-- Calendar UI preferences. Empty-day placeholders default to ON (1) so existing
-- users see them after upgrade; it's the more useful state for sparse weeks.
ALTER TABLE user_preferences ADD COLUMN calendar_show_empty_days INTEGER NOT NULL DEFAULT 1;
