-- Pending overlay applications, used by initial-pair hydration (T-011).
--
-- When a fresh device pairs to an existing seed, the Loro doc may carry
-- overlay entries (e.g. `course_overlays.<id>.is_pinned`) for rows that
-- haven't been fetched from Brightspace yet. The bridge can't apply
-- those overlays straight away — there's no row to UPDATE — so it
-- parks them here keyed by `(kind, key)`. Each Brightspace fetcher
-- (sync/courses.rs, sync/assignments.rs, …) calls
-- `bridge::drain_pending_overlay` after upserting a fresh row so any
-- pending overlay for that key gets re-applied on top.
--
-- `kind` mirrors the §10 overlay namespace; `key` mirrors the lookup
-- key used by the matching SQLite table (org_unit_id, "course_id:bid",
-- brightspace_id, external_id, …). `payload` is the JSON snapshot of
-- the overlay fields at hydrate time — drain re-applies that snapshot
-- once the underlying row exists. apply_remote may overwrite later if
-- the peer's state has moved on.

CREATE TABLE IF NOT EXISTS pending_overlay_apply (
  kind        TEXT NOT NULL,  -- 'course' | 'assignment' | 'grade' | 'content_item' | 'notification'
  key         TEXT NOT NULL,
  payload     TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (kind, key)
);
