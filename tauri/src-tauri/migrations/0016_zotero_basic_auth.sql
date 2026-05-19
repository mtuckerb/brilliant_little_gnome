-- Separate basic-auth credentials from the Zotero API key. Earlier we
-- doubled zotero_user_id / zotero_api_key as both Zotero credentials and
-- reverse-proxy Basic Auth — which works until you need both (Tucker's
-- proxy needs Basic Auth AND the upstream Zotero needs an API key).
-- New columns hold the proxy credentials; zotero_user_id / zotero_api_key
-- go back to being the Zotero ones.
ALTER TABLE user_preferences ADD COLUMN zotero_basic_auth_user TEXT;
ALTER TABLE user_preferences ADD COLUMN zotero_basic_auth_pass TEXT;
