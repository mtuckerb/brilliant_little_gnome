-- Default to the desktop Zotero's local HTTP server (Tucker's setup).
-- Cloud Zotero is opt-in and additionally requires zotero_user_id +
-- zotero_api_key, which were added in migration 0011.
ALTER TABLE user_preferences ADD COLUMN zotero_use_local INTEGER NOT NULL DEFAULT 1;
