-- Optional override for the Zotero user ID in local mode. Defaults to
-- NULL (which means "use 0"). Useful when a reverse-proxied Zotero
-- (Tucker's setup at zotero.tuckerbradford.com) rejects the canonical
-- local "0" and expects the user's real Zotero user ID instead. Keeps
-- Basic Auth credentials separate from the API user ID.
ALTER TABLE user_preferences ADD COLUMN zotero_local_user_id TEXT;
