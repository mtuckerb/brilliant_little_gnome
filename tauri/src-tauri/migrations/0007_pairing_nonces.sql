-- One-shot pairing nonces (T-012, design.md §6 + §12 threat model).
--
-- The QR payload carries a 16-byte nonce alongside the secret. The
-- joining device sends the full payload back to the seed; the seed
-- accepts the pairing only if the nonce hasn't been seen before, and
-- records it here. A screenshot replay one minute later fails because
-- the nonce is now in this table — even within the 60s expiry window.
--
-- We never garbage-collect this table: 16 bytes per pairing is cheap
-- forever, and any GC opens a replay window the size of the GC
-- interval. Long-tail growth is bounded by the number of times a user
-- ever scans a QR (rotation invalidates by changing the secret).

CREATE TABLE IF NOT EXISTS consumed_pairing_nonces (
  nonce        TEXT PRIMARY KEY,
  consumed_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
