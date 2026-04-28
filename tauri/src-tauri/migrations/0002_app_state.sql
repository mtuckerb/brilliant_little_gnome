-- Generic small key/value store for app-wide flags (cooldown timestamps,
-- one-shot setup markers, etc.). Modelled after the Ruby UserPreference.get/set
-- pattern used by Brilliant::Sync::Psy220ScraperService.

CREATE TABLE IF NOT EXISTS app_state (
  key TEXT PRIMARY KEY,
  value TEXT,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
