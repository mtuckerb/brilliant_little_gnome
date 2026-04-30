-- P2P device-to-device sync (T-002, design.md §6 + §13).
--
-- p2p_enabled gates the engine entirely; defaults to 0 so existing
-- installs remain dark until the user opts in via Settings → Sync.
--
-- Secret material (iroh_node_secret, sync_doc_secret, sync_doc_id) is
-- NOT inserted here. The original ticket sketch tried to seed NULL
-- rows into app_state, but that pattern is awkward — instead, the
-- p2p_enable command (added in T-014) does INSERT OR REPLACE on first
-- use. T-021 will move these into the OS keychain where available.

ALTER TABLE user_preferences ADD COLUMN p2p_enabled INTEGER NOT NULL DEFAULT 0;
