-- Paired-device roster for Phase 2 pairing UX.
--
-- The transport can operate from the shared sync secret alone, but the UI and
-- follow-up reconciliation work need durable knowledge of which device IDs have
-- been paired before. We store the iroh EndpointId/NodeId as both `id` and
-- `public_key` for now: iroh's endpoint identifier is derived from the device
-- public key and is the stable display/dial identity used by the QR payload.
--
-- Secrets are intentionally excluded. The shared sync_doc_secret remains in the
-- keychain/app_state backend, and Brightspace cookies are never replicated.

CREATE TABLE IF NOT EXISTS paired_devices (
  id            TEXT PRIMARY KEY,
  public_key    TEXT NOT NULL,
  last_seen_at  TEXT,
  label         TEXT,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_paired_devices_last_seen
  ON paired_devices(last_seen_at DESC);
