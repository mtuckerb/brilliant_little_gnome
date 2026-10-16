# Students directory

Scope: add a local SQLite students table and permanent course membership history,
backfill saved rosters, import learners during roster sync, and expose a searchable
Students table from desktop and mobile navigation. Store imported and custom
avatars in SQLite, with custom edits taking precedence on every later import.
Existing class lists should display the same saved avatar.

Acceptance criteria:
- Each learner encountered in any saved class appears once, including archived
  classes; instructors and preview accounts are excluded.
- Roster changes never erase a previously encountered learner or membership.
- A user can upload, replace, or remove an avatar; the choice survives restart,
  offline use, and later roster refreshes.
- Existing rosters backfill automatically; importing all classes can tolerate
  inaccessible classes while preserving their saved history.
- Frontend build/tests and Rust persistence regression tests pass.

Out of scope: legacy Ruby UI, remote profile editing, and student data P2P sync.

The initial photo import uses Brightspace's profile-image endpoint and keeps a
bounded raster data URL locally. Permissions or missing photos fall back to
initials. Uploading or removing a photo sets a permanent local override, including
an explicit blank override so a later import cannot restore a removed photo.
