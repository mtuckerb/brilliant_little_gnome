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

Release review for 2.0.24:
- Integrates with the current uncached roster refresh API and uses migration 0024,
  after the shipped schedule and syllabus migrations.
- Keeps a saved roster successful even if optional avatar imports fail. Temporary
  photo-download failures remain eligible for retry on a later roster refresh.
- Keychain integration tests use a separate test service and serialize their
  shared slots, so verification cannot modify the installed app's sync keys.
- Passed 111 frontend tests, 174 Rust tests, frontend build, configuration checks,
  and migration-number validation. A copy of the installed 2.0.23 database upgraded
  to 569 unique learner records with foreign keys and integrity intact.
