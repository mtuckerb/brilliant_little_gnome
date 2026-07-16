-- Spotify Web API app credentials, used to read course playlist tracklists
-- (e.g. MUH-105's unit playlists) so they can be taken to Apple Music.
-- Device-local like the Zotero credentials: never mirrored into Loro, so a
-- paired peer never receives the secret.
ALTER TABLE user_preferences ADD COLUMN spotify_client_id TEXT;
ALTER TABLE user_preferences ADD COLUMN spotify_client_secret TEXT;
