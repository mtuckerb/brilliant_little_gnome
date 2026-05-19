-- Optional override for the local Zotero base URL. Defaults to NULL, which
-- means "use the standard local endpoint" (http://127.0.0.1:23119/api).
-- Useful when the user is reverse-proxying / tunnelling their Zotero
-- desktop to a custom hostname (e.g. zotero.example.com).
ALTER TABLE user_preferences ADD COLUMN zotero_local_base_url TEXT;
