# Notification System Fixes

## TL;DR

> **Quick Summary**: Fix four interrelated notification problems — a date sorting bug, over-fetching on all courses, incomplete course-level notification views, and a new per-course freeze toggle — without touching unrelated sync infrastructure.
>
> **Deliverables**:
> - `upsert_notification_batch` uses canonical Brightspace date (StartDate preferred, LastModifiedDate for edits)
> - Frozen courses skipped across ALL sync loops (notifications, grades, assignments, discussions, TOC)
> - `courses/<id>/notifications` shows News items in addition to Content updates
> - `is_frozen` DB column + freeze toggle UI on course card
>
> **Estimated Effort**: Medium
> **Parallel Execution**: YES — 3 waves
> **Critical Path**: Task 1 (migration) → Task 2 (model/service freeze guard) → Task 5 (UI toggle)

---

## Context

### Original Request
Notifications are a mess in three ways:
1. The app constantly downloads notifications for closed/ended courses — wasteful and risks rate limiting
2. Notification dates don't reflect the actual Brightspace post date — old notifications bubble to the top because `upsert_notification_batch` takes `parsed_dates.max`, which lets a future `start_date` beat the actual post date
3. `/notifications` (global feed) shows many types but `courses/<id>/notifications` only shows Content updates — News items and other types are missing from the course-scoped view

Plus a user-requested feature: a per-course "frozen" checkbox to permanently stop all syncing.

### Interview Summary
**Key Discussions**:
- **Canonical date**: StartDate (when announcement becomes visible to students), then LastModifiedDate (if edited, bubble up), then CreatedDate as final fallback
- **Freeze scope**: ALL syncing — notifications, grades, assignments, discussions, TOC, AND assignment deadline notifications from local DB
- **Freeze UI location**: Course card, same area as drop/withdraw controls
- **`ends_at` auto-skip**: Out of scope for this plan — store the field if convenient, do not act on it

**Research Findings**:
- `upsert_notification_batch` date candidates: `[date, last_modified, created_at, start_date].compact` → `.max` — BUG confirmed
- `sync_course_specific_notifications` iterates up to 60 courses with no active/frozen check — confirmed
- News items ARE already synced per-course to the DB; gap is in the API endpoint/view layer for course-scoped notification queries
- `sync_upcoming_assignment_notifications` queries `Assignment` table directly — bypasses course enrollment list, needs explicit freeze guard
- Drop/withdraw pattern in `course_controller.rb` POST `/course/:id/drop` is the direct template for the freeze endpoint

### Metis Review
**Identified Gaps (addressed)**:
- Fix #3 was misidentified as a sync gap — it's an API/view layer gap; News items are in the DB already
- Date correction downward will trigger `date_diff > 3600` → mark unread; need to gate re-unread on direction (downward correction should NOT mark unread)
- `sync_upcoming_assignment_notifications` explicitly needs freeze guard — it bypasses the course list
- `courses.take(60)` limit ordering: frozen courses should be filtered BEFORE the take(60), not after, so they don't consume slots

---

## Work Objectives

### Core Objective
Correct notification date sorting, eliminate wasteful syncing of frozen/unwanted courses, complete the course-scoped notification view, and provide a UI toggle to freeze a course.

### Concrete Deliverables
- `db/migrate/YYYYMMDDHHMMSS_add_is_frozen_to_courses.rb` — new boolean column, default false
- `models/course.rb` — `frozen?` convenience method
- `lib/brilliant/sync/notification_service.rb` — date fix + freeze guards
- `lib/brilliant/client.rb` — freeze guards in all proactive sync loops
- `controllers/course_controller.rb` — POST `/course/:id/freeze` endpoint
- `controllers/api/v1/api_controller.rb` — course-scoped notifications includes News type
- `views/` — freeze toggle checkbox on course card

### Definition of Done
- [ ] A news item with future StartDate sorts by StartDate (not CreatedDate), but an edited item sorts by LastModifiedDate
- [ ] A frozen course produces zero API calls in the next 30-minute sync cycle (verified via log inspection)
- [ ] `/course/:id/notifications` (or the API query it uses) returns both Content AND News type records for that course_id
- [ ] Freeze checkbox visible on course card; toggle persists across app restart

### Must Have
- `is_frozen` defaults to `false` (never null) — use `unless column_exists?` in migration
- Frozen filter applied BEFORE `courses.take(60)` — frozen courses do not consume sync slots
- Date correction downward does NOT re-mark notification as unread
- Freeze is reversible — toggling off resumes syncing on next cycle

### Must NOT Have (Guardrails)
- Do NOT touch `ends_at` auto-skip logic — store the field only if it falls naturally from migration, do not gate sync on it
- Do NOT redesign course card layout — freeze checkbox only, no visual restructuring
- Do NOT touch the deduplication logic in `get_content_notifications` — leave `seen_ids` mechanism alone
- Do NOT add News pagination handling
- Do NOT add freeze guards to unrelated non-sync code paths (auth, settings, etc.)
- Do NOT change the `status` field semantics — frozen is a separate `is_frozen` boolean, not a new status value

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: NO (no test framework detected)
- **Automated tests**: None — no test infrastructure setup in scope
- **Agent-Executed QA**: ALWAYS — every task verified via Bash (curl/sqlite3/log grep) or browser (Playwright)

### QA Policy
- **API/sync verification**: Bash (curl + sqlite3 + log grep)
- **UI verification**: Playwright (browser automation)
- **Evidence**: `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — foundation, can all run in parallel):
├── Task 1: DB migration — add is_frozen to courses [quick]
├── Task 2: Date fix — upsert_notification_batch canonical date logic [quick]
└── Task 3: Course-scoped notification view fix — include News type [quick]

Wave 2 (After Task 1 — freeze guards, all parallel):
├── Task 4: notification_service.rb freeze guards + take(60) reorder [quick]
└── Task 5: client.rb freeze guards — all proactive sync loops [quick]

Wave 3 (After Tasks 1+4+5 — UI, sequential on model):
├── Task 6: course_controller.rb freeze endpoint [quick]
└── Task 7: Freeze toggle UI on course card [visual-engineering]

Wave FINAL (After ALL — parallel review):
├── Task F1: Plan compliance audit [oracle]
├── Task F2: Sync behavior QA — log inspection + sqlite verify [unspecified-high]
└── Task F3: UI QA — Playwright freeze toggle [unspecified-high]
```

**Critical Path**: Task 1 → Task 4 → Task 6 → Task 7
**Parallel Speedup**: ~60% faster than sequential
**Max Concurrent**: 3 (Wave 1)

### Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| 1 | — | 4, 5, 6, 7 |
| 2 | — | — |
| 3 | — | — |
| 4 | 1 | F2 |
| 5 | 1 | F2 |
| 6 | 1 | 7 |
| 7 | 1, 6 | F3 |
| F1 | all | — |
| F2 | 4, 5 | — |
| F3 | 7 | — |

### Agent Dispatch Summary
- **Wave 1**: T1 → `quick`, T2 → `quick`, T3 → `quick`
- **Wave 2**: T4 → `quick`, T5 → `quick`
- **Wave 3**: T6 → `quick`, T7 → `visual-engineering`
- **FINAL**: F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`

---

## TODOs

---


- [x] 1. Add `is_frozen` column to courses table

  **What to do**:
  - Create `db/migrate/YYYYMMDDHHMMSS_add_is_frozen_to_courses.rb`
  - Use `unless column_exists?(:courses, :is_frozen)` guard (established pattern)
  - `add_column :courses, :is_frozen, :boolean, default: false, null: false`
  - Run migration
  - Add `frozen?` convenience method to `models/course.rb`: `def frozen? = is_frozen == true`
  - Note: `frozen?` is a reserved Ruby method name — use `sync_frozen?` or `is_frozen?` instead

  **Must NOT do**:
  - Do NOT add an `ends_at` column or any other new column beyond `is_frozen`
  - Do NOT change the `status` field or its existing values

  **Recommended Agent Profile**:
  > Migration + model method — no UI, no complex logic.
  - **Category**: `quick`
    - Reason: Single-file migration + one-line model method
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2 and 3)
  - **Blocks**: Tasks 4, 5, 6, 7
  - **Blocked By**: None (can start immediately)

  **References**:
  - `db/migrate/20260214000000_add_status_to_courses.rb` — existing migration pattern (use `unless column_exists?`)
  - `models/course.rb` — existing `dropped?` and `fail_on_drop?` methods as pattern for boolean convenience methods

  **Acceptance Criteria**:

  - [ ] `sqlite3 brilliant.db ".schema courses"` output includes `is_frozen INTEGER DEFAULT 0 NOT NULL`
  - [ ] `Course.first.respond_to?(:is_frozen?)` returns true
  - [ ] `Course.first.is_frozen?` returns false on an unfrozen record

  **QA Scenarios**:
  ```
  Scenario: Column exists with correct default
    Tool: Bash
    Preconditions: App started, migration run
    Steps:
      1. Run: sqlite3 brilliant.db ".schema courses" | grep is_frozen
      2. Assert output contains: is_frozen
      3. Run: sqlite3 brilliant.db "SELECT is_frozen FROM courses LIMIT 1"
      4. Assert output: 0
    Expected Result: Column present, default value is 0 (false)
    Evidence: .sisyphus/evidence/task-1-schema-verify.txt

  Scenario: Frozen? method absent on Ruby reserved name
    Tool: Bash
    Steps:
      1. Run: bundle exec ruby -e "require './models/course'; puts Course.instance_methods(false).grep(/frozen/)"
      2. Assert: no conflict with Ruby's built-in Object#frozen?
    Expected Result: Method named is_frozen? or sync_frozen? exists and returns boolean
    Evidence: .sisyphus/evidence/task-1-method-verify.txt
  ```

  **Commit**: YES (Wave 1)
  - Message: `feat(courses): add is_frozen boolean column to courses table`
  - Files: `db/migrate/*_add_is_frozen_to_courses.rb`, `models/course.rb`


- [ ] 2. Fix notification date sorting in `upsert_notification_batch`

  **What to do**:
  - Open `lib/brilliant/sync/notification_service.rb`
  - Find the `upsert_notification_batch` method, specifically the date selection block (lines ~75-78):
    ```ruby
    # CURRENT (buggy):
    dates = [data[:date], data[:last_modified], data[:created_at], data[:start_date]].compact
    parsed_dates = dates.map { |d| Time.zone.parse(d.to_s) rescue nil }.compact
    best_date = parsed_dates.max
    ```
  - Replace with priority-based selection:
    ```ruby
    # NEW: StartDate preferred (visibility date), LastModifiedDate second (edits bubble up),
    # CreatedDate as final fallback. start_date removed from candidates entirely.
    candidate_fields = [data[:start_date], data[:last_modified], data[:date], data[:created_at]]
    best_date = candidate_fields
      .map { |d| d.present? ? (Time.zone.parse(d.to_s) rescue nil) : nil }
      .compact
      .first
    ```
  - Fix the re-unread guard: currently `date_diff > 3600` marks is_read=false regardless of direction.
    Change to only mark unread when date moves FORWARD (not backward — date corrections shouldn't re-unread):
    ```ruby
    # CURRENT:
    date_changed = existing_date && (best_date - existing_date).abs > 3600
    # NEW:
    date_changed = existing_date && best_date && (best_date - existing_date) > 3600
    # (only positive delta > 1hr triggers unread, not downward corrections)
    ```
  - Also update `sync_course_specific_notifications` news item mapping (~line 217) to match:
    The `:date` field should stay as `item['StartDate'] || item['LastModifiedDate'] || item['CreatedDate']`
    The `:last_modified` field stays as `item['LastModifiedDate']`
    **Remove** `:start_date` as a separate key passed to `upsert_notification_batch` for news items
    (it was redundant and caused the `.max` bug)

  **Must NOT do**:
  - Do NOT change the date logic for other notification types (unified feed, global alerts, content) unless they also pass `:start_date` — verify each caller
  - Do NOT change the `upsert_all` SQL or the `unique_by` clause
  - Do NOT alter the `content_changed` detection logic

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Targeted logic change in one method, ~10 lines
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1 and 3)
  - **Blocks**: Nothing directly (improvement takes effect on next sync)
  - **Blocked By**: None

  **References**:
  - `lib/brilliant/sync/notification_service.rb:75-78` — current buggy date selection block
  - `lib/brilliant/sync/notification_service.rb:210-225` — news item mapping where `:start_date` key is set
  - `lib/brilliant/sync/notification_service.rb:90-100` — `date_changed` guard and is_read reset logic
  - `lib/brilliant/client.rb:344-425` — `get_content_notifications()` to verify it does NOT pass `:start_date` (check before removing)

  **Acceptance Criteria**:

  - [ ] A news item with `start_date: '2026-04-01'`, `last_modified: '2026-02-10'`, `created_at: '2026-01-15'` gets `best_date = 2026-04-01` (StartDate wins)
  - [ ] A news item with `start_date: nil`, `last_modified: '2026-02-10'`, `created_at: '2026-01-15'` gets `best_date = 2026-02-10` (LastModifiedDate fallback)
  - [ ] An existing notification whose stored date is `2026-04-01` receiving an update with `best_date = 2026-02-10` does NOT have `is_read` set to false
  - [ ] An existing notification receiving an update with `best_date` 2+ hours LATER than stored does set `is_read = false`

  **QA Scenarios**:
  ```
  Scenario: StartDate wins over CreatedDate
    Tool: Bash (sqlite3 + manual upsert call in irb/console)
    Preconditions: App running, a test notification can be upserted
    Steps:
      1. In app console: call upsert_notification_batch with data including start_date: '2026-04-01', created_at: '2026-01-15'
      2. Query: sqlite3 brilliant.db "SELECT date FROM notifications WHERE external_id='test-1'"
      3. Assert: date == 2026-04-01
    Expected Result: date field equals StartDate value
    Evidence: .sisyphus/evidence/task-2-date-priority.txt

  Scenario: Downward date correction does not mark unread
    Tool: Bash
    Steps:
      1. Insert notification with date = '2026-04-01', is_read = 1
      2. Call upsert with same external_id, best_date = '2026-02-01' (2 months earlier)
      3. Query is_read for that notification
      4. Assert: is_read = 1 (still read, not reset)
    Expected Result: is_read unchanged on backward date correction
    Evidence: .sisyphus/evidence/task-2-no-unread-on-correction.txt
  ```

  **Commit**: YES (Wave 1)
  - Message: `fix(notifications): use StartDate→LastModifiedDate→CreatedDate priority; fix backward-date unread regression`
  - Files: `lib/brilliant/sync/notification_service.rb`


- [ ] 3. Include News type in course-scoped notification queries

  **What to do**:
  - Find where `GET /api/v1/notifications?course_id=X` is handled — likely `controllers/api/v1/api_controller.rb`
  - Inspect the query: it likely filters by `course_id` but may also filter `notification_type` or have other restrictions that exclude News
  - If there's a type filter excluding News, remove it or add News to the allowed types
  - Check `views/course_notifications.erb` (or equivalent) — if it has a hard-coded type filter in the view/JS, update it too
  - The fix should be minimal: News items are already in the DB for each course_id — just ensure the query returns them
  - Verify the filter UI (if any) has a 'News' option checkbox — if missing, add it alongside Content/Grade/Assignment

  **Must NOT do**:
  - Do NOT add a new sync path — News items are already synced, this is a query/display fix only
  - Do NOT change the notification schema or add new fields
  - Do NOT touch the unified `/notifications` endpoint (global feed) — it already works

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Query filter change + possible view update, no new data pipeline
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1 and 2)
  - **Blocks**: Nothing
  - **Blocked By**: None

  **References**:
  - `controllers/api/v1/api_controller.rb` — find the `/api/v1/notifications` GET endpoint
  - `views/course_notifications.erb` or `views/notifications.erb` — check for type filter logic
  - `models/notification.rb` — verify `belongs_to :course` (foreign_key: course_id) for query construction

  **Acceptance Criteria**:

  - [ ] `curl 'http://localhost:4567/api/v1/notifications?course_id=REAL_ID'` returns records with `notification_type: 'News'` when News items exist for that course
  - [ ] No duplicate notifications appear (same item listed twice)
  - [ ] Content type notifications still appear (no regression)

  **QA Scenarios**:
  ```
  Scenario: Course-scoped query returns News items
    Tool: Bash (curl)
    Preconditions: At least one News notification exists in DB for a known course_id
    Steps:
      1. Run: sqlite3 brilliant.db "SELECT COUNT(*) FROM notifications WHERE course_id='COURSE_ID' AND notification_type='News'"
      2. Assert: count > 0 (confirming test data exists)
      3. Run: curl -s 'http://localhost:4567/api/v1/notifications?course_id=COURSE_ID'
      4. Parse response: ruby -rjson -e 'puts JSON.parse(STDIN.read).map{|n| n["notification_type"]}.uniq'
      5. Assert: output includes both 'News' and 'Content'
    Expected Result: Both types present in response
    Evidence: .sisyphus/evidence/task-3-course-notif-types.txt

  Scenario: No duplicates
    Tool: Bash
    Steps:
      1. Run curl as above, pipe to: ruby -rjson -e 'data=JSON.parse(STDIN.read); puts data.map{|n| n["external_id"]}.tally.select{|_,v| v>1}'
      2. Assert: empty output (no duplicate external_ids)
    Expected Result: Zero duplicates
    Evidence: .sisyphus/evidence/task-3-no-duplicates.txt
  ```

  **Commit**: YES (Wave 1)
  - Message: `fix(notifications): include News type in course-scoped notification queries`
  - Files: `controllers/api/v1/api_controller.rb`, possibly `views/course_notifications.erb`


- [ ] 4. Add freeze guards to `notification_service.rb` + fix take(60) ordering

  **What to do**:
  - Open `lib/brilliant/sync/notification_service.rb`
  - In `sync_course_specific_notifications()`, change course filtering:
    ```ruby
    # CURRENT:
    courses_to_sync = limit ? courses.take(limit) : (full_sync ? courses : courses.take(60))
    # NEW: reject frozen BEFORE taking the limit so frozen courses don't consume slots
    active_courses = courses.reject(&:is_frozen?)
    courses_to_sync = limit ? active_courses.take(limit) : (full_sync ? active_courses : active_courses.take(60))
    ```
  - In `sync_upcoming_assignment_notifications()` (in `client.rb` — see Task 5 for that), but also check if this method is called from `notification_service.rb` directly — if so add guard here too
  - Add a log line when a course is skipped: `Rails.logger.debug "Skipping frozen course #{course.name} (#{course.org_unit_id})"` (or equivalent logger)

  **Must NOT do**:
  - Do NOT gate the global feed sync (`get_unified_feed`) — that's a single cross-course API call, not per-course
  - Do NOT gate `get_global_alerts` — same reason
  - Do NOT change the `sync_enrollments` call — frozen courses should still update their enrollment metadata

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: One-line filter change + log line
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 5)
  - **Parallel Group**: Wave 2
  - **Blocks**: F2
  - **Blocked By**: Task 1 (needs `is_frozen` column)

  **References**:
  - `lib/brilliant/sync/notification_service.rb:194-265` — `sync_course_specific_notifications` method, `courses_to_sync` assignment at top
  - `lib/brilliant/sync/notification_service.rb` — `sync_upcoming_assignment_notifications` — check if it uses course list or queries Assignment table directly

  **Acceptance Criteria**:

  - [ ] Freeze course X in DB: `sqlite3 brilliant.db "UPDATE courses SET is_frozen=1 WHERE org_unit_id='X'"`
  - [ ] Trigger sync manually (or wait for 30-min cycle)
  - [ ] `grep 'org_unit_id_of_X' log/development.log` shows zero news/content API calls for course X after freeze
  - [ ] Non-frozen courses still sync normally (no regression)

  **QA Scenarios**:
  ```
  Scenario: Frozen course skipped in per-course sync
    Tool: Bash
    Preconditions: App running, a course exists with known org_unit_id
    Steps:
      1. sqlite3 brilliant.db "UPDATE courses SET is_frozen=1 WHERE org_unit_id='TEST_ID'"
      2. Trigger a sync (POST /sync or wait for cycle)
      3. grep log/development.log for 'TEST_ID' API call patterns
      4. Assert: no /news/ or /content/updates calls for TEST_ID
    Expected Result: Zero per-course API calls for frozen course
    Evidence: .sisyphus/evidence/task-4-freeze-skip-log.txt

  Scenario: Frozen course does not consume take(60) slot
    Tool: Bash
    Steps:
      1. Freeze courses 1-60 in DB
      2. Ensure course 61 exists (unfrozen)
      3. Trigger sync
      4. Grep logs for course 61's org_unit_id
      5. Assert: course 61 WAS synced (it was beyond slot 60 of total, but frozen courses freed up slots)
    Expected Result: Course 61 syncs because frozen courses don't count toward the 60-slot limit
    Evidence: .sisyphus/evidence/task-4-slot-ordering.txt
  ```

  **Commit**: YES (Wave 2)
  - Message: `fix(notifications): skip frozen courses in per-course sync; freeze filter before take(60)`
  - Files: `lib/brilliant/sync/notification_service.rb`


- [ ] 5. Add freeze guards to all proactive sync loops in `client.rb`

  **What to do**:
  - Open `lib/brilliant/client.rb`
  - Find `sync_all_courses_proactively()` — the 30-min background sync method
  - Add freeze filter before each per-course loop:
    - TOC sync loop: `courses.reject(&:is_frozen?).each do |course|`
    - Assignments sync loop: same
    - Discussions sync loop: same
    - Grades sync loop: same
    - Quizzes sync loop: same (if present)
  - Find `sync_upcoming_assignment_notifications()` — this queries `Assignment` table directly
    - Add guard: `courses_to_notify = courses.reject(&:is_frozen?)` and filter assignments by those course_ids
    - If the method doesn't take a course list, it needs to load `Course.where(is_frozen: false)` for the filter
  - Find the per-course background threads in `GET /api/v1/courses/:id/summary` handler
    - Add `next if course.is_frozen?` before spawning background sync threads for that course

  **Must NOT do**:
  - Do NOT gate the initial `get_enrollments()` call — frozen courses still appear in the course list
  - Do NOT gate global/cross-course API calls (unified feed, global alerts)
  - Do NOT add frozen checks to auth or cookie management code

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Pattern-repetitive guard additions across several loops
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 4)
  - **Parallel Group**: Wave 2
  - **Blocks**: F2
  - **Blocked By**: Task 1 (needs `is_frozen` column)

  **References**:
  - `lib/brilliant/client.rb:1-120` — 30-min sync thread + `sync_all_courses_proactively` definition
  - `lib/brilliant/client.rb` — `sync_upcoming_assignment_notifications` — confirm it queries Assignment table and identify how to inject course filter
  - `controllers/api/v1/api_controller.rb:100-179` — `GET /api/v1/courses/:id/summary` background thread pattern
  - `models/course.rb` — `is_frozen?` method (added in Task 1)

  **Acceptance Criteria**:

  - [ ] For a frozen course, triggering the 30-min proactive sync produces zero TOC, assignment, grade, discussion API calls for that course (grep log)
  - [ ] `sync_upcoming_assignment_notifications` does not surface assignment deadline notifications for frozen courses
  - [ ] Navigating to a frozen course's summary page does NOT trigger background sync threads for it
  - [ ] Unfreezing a course and triggering sync resumes all loops for it

  **QA Scenarios**:
  ```
  Scenario: All proactive sync loops skip frozen course
    Tool: Bash
    Preconditions: Course X frozen in DB, app running
    Steps:
      1. Trigger full proactive sync (wait for 30-min cycle or POST /sync?full=true)
      2. Run: grep 'COURSE_X_ORG_UNIT_ID' log/development.log | grep -v enrollment | tail -50
      3. Assert: zero lines (no API calls except enrollment)
    Expected Result: Frozen course produces no per-course sync API calls
    Evidence: .sisyphus/evidence/task-5-all-loops-frozen.txt

  Scenario: Assignment deadline notification not surfaced for frozen course
    Tool: Bash
    Steps:
      1. Ensure an upcoming assignment exists for frozen course X in Assignment table
      2. Trigger sync
      3. sqlite3 brilliant.db "SELECT COUNT(*) FROM notifications WHERE course_id='X' AND notification_type='Assignment' AND date > datetime('now')"
      4. Assert: count has not increased since freeze was set
    Expected Result: No new assignment deadline notifications for frozen courses
    Evidence: .sisyphus/evidence/task-5-assignment-deadlines-frozen.txt
  ```

  **Commit**: YES (Wave 2)
  - Message: `fix(sync): skip all proactive sync loops for frozen courses including assignment deadlines`
  - Files: `lib/brilliant/client.rb`


- [ ] 6. Add `POST /course/:id/freeze` endpoint

  **What to do**:
  - Open `controllers/course_controller.rb`
  - Model directly after the existing `POST /course/:id/drop` endpoint
  - Add a new route:
    ```ruby
    post '/course/:id/freeze' do
      course = Course.find_by(org_unit_id: params[:id])
      halt 404, { error: 'Course not found' }.to_json unless course
      frozen = params[:frozen] == 'true' || params[:frozen] == true
      course.update!(is_frozen: frozen)
      if request.xhr?
        content_type :json
        { status: 'ok', is_frozen: course.is_frozen? }.to_json
      else
        redirect back
      end
    end
    ```
  - No auth changes needed — follows same session/auth pattern as existing endpoints

  **Must NOT do**:
  - Do NOT change the `status` field in this endpoint
  - Do NOT add bulk-freeze functionality (single course only)
  - Do NOT add a GET endpoint — the frozen state is readable from `GET /api/v1/courses/:id`

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Sinatra route, ~15 lines, direct copy of drop pattern
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 7 setup, but Task 7 needs this to exist first)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 7
  - **Blocked By**: Task 1

  **References**:
  - `controllers/course_controller.rb` — `POST /course/:id/drop` endpoint (lines ~295-320) — copy this pattern exactly
  - `models/course.rb` — `is_frozen?` method and `update!` usage

  **Acceptance Criteria**:

  - [ ] `curl -X POST 'http://localhost:4567/course/COURSE_ID/freeze' -d 'frozen=true'` returns `{"status":"ok","is_frozen":true}`
  - [ ] `sqlite3 brilliant.db "SELECT is_frozen FROM courses WHERE org_unit_id='COURSE_ID'"` returns 1
  - [ ] `curl -X POST ... -d 'frozen=false'` returns `{"is_frozen":false}` and DB resets to 0
  - [ ] Non-existent course_id returns HTTP 404

  **QA Scenarios**:
  ```
  Scenario: Freeze a course via POST
    Tool: Bash (curl)
    Preconditions: App running, a course exists with known org_unit_id
    Steps:
      1. curl -s -X POST 'http://localhost:4567/course/COURSE_ID/freeze' -d 'frozen=true'
      2. Assert HTTP response contains: {"status":"ok","is_frozen":true}
      3. sqlite3 brilliant.db "SELECT is_frozen FROM courses WHERE org_unit_id='COURSE_ID'"
      4. Assert: 1
    Expected Result: Course frozen in DB, endpoint returns confirmation
    Evidence: .sisyphus/evidence/task-6-freeze-endpoint.txt

  Scenario: Unfreeze a course
    Tool: Bash (curl)
    Steps:
      1. (Course already frozen from above scenario)
      2. curl -s -X POST 'http://localhost:4567/course/COURSE_ID/freeze' -d 'frozen=false'
      3. Assert response: {"is_frozen":false}
      4. sqlite3 query: assert 0
    Expected Result: Course unfrozen successfully
    Evidence: .sisyphus/evidence/task-6-unfreeze-endpoint.txt

  Scenario: 404 for unknown course
    Tool: Bash (curl)
    Steps:
      1. curl -s -o /dev/null -w "%{http_code}" -X POST 'http://localhost:4567/course/99999999/freeze' -d 'frozen=true'
      2. Assert: HTTP 404
    Expected Result: 404 returned for non-existent course
    Evidence: .sisyphus/evidence/task-6-404-unknown.txt
  ```

  **Commit**: YES (Wave 3, group with Task 7)
  - Message: `feat(courses): add freeze/unfreeze endpoint POST /course/:id/freeze`
  - Files: `controllers/course_controller.rb`


- [ ] 7. Add freeze toggle checkbox to course card UI

  **What to do**:
  - Find the course card view partial — likely `views/course_card.erb`, `views/partials/_course_card.erb`, or equivalent
  - Locate the drop/withdraw controls section (same area as Task 6's endpoint model)
  - Add a freeze toggle: an HTML checkbox + label, styled to match existing drop UI controls
  - Wire it with a small JS fetch call (matching existing XHR pattern):
    ```javascript
    // On checkbox change:
    fetch(`/course/${courseId}/freeze`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded', 'X-Requested-With': 'XMLHttpRequest' },
      body: `frozen=${checkbox.checked}`
    })
    ```
  - Add a visual indicator for frozen state: a small ice/snowflake emoji (❄️) or badge next to the course name, visible when `course.is_frozen?`
  - The checkbox should render as checked when `course.is_frozen?` is true (server-side ERB conditional)
  - Match the loading/confirmation feedback style of the existing drop toggle

  **Must NOT do**:
  - Do NOT redesign the course card layout beyond adding the checkbox + badge
  - Do NOT hide frozen courses from the course list — they remain visible
  - Do NOT add a separate settings page for freeze management

  **Recommended Agent Profile**:
  - **Category**: `visual-engineering`
    - Reason: ERB partial + JS wiring + CSS matching existing style
  - **Skills**: [`frontend-ui-ux`]
    - `frontend-ui-ux`: Matching existing UI patterns, checkbox styling, feedback UX

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3, sequential after Task 6
  - **Blocks**: F3
  - **Blocked By**: Tasks 1, 6

  **References**:
  - `views/` — find the course card partial by searching for the drop/withdraw UI markup
  - `controllers/course_controller.rb` — drop endpoint XHR response pattern (JS side mirrors the drop toggle's fetch call)
  - `models/course.rb` — `is_frozen?` method for ERB conditional rendering
  - Existing drop toggle JS in views — copy the fetch/XHR pattern exactly, change endpoint to `/freeze`

  **Acceptance Criteria**:

  - [ ] Freeze checkbox visible on course card for all courses
  - [ ] Checkbox is checked when `course.is_frozen == true` on page load (server-rendered state)
  - [ ] Clicking checkbox fires POST to `/course/:id/freeze` with correct `frozen` param
  - [ ] After toggle, ❄️ badge appears/disappears on course name without page reload
  - [ ] Reloading the page preserves the frozen state (checkbox still checked)
  - [ ] Unfreezing works (uncheck → POST frozen=false → badge disappears)

  **QA Scenarios**:
  ```
  Scenario: Freeze checkbox appears and toggles on
    Tool: Playwright
    Preconditions: App running, at least one course visible on course list page
    Steps:
      1. Navigate to http://localhost:4567/courses (or equivalent course list URL)
      2. Locate a course card — find checkbox with label containing 'Freeze' or 'Frozen'
      3. Assert: checkbox exists and is unchecked (course not frozen)
      4. Click the checkbox
      5. Wait for network request to /course/ID/freeze to complete (timeout: 5s)
      6. Assert: checkbox is now checked
      7. Assert: ❄️ icon visible near course name
    Expected Result: Toggle fires correctly, UI updates without reload
    Evidence: .sisyphus/evidence/task-7-freeze-toggle-on.png

  Scenario: Frozen state persists on page reload
    Tool: Playwright
    Preconditions: Course X is frozen (is_frozen=1 in DB)
    Steps:
      1. Navigate to course list page
      2. Find course X's card
      3. Assert: freeze checkbox is checked (rendered from server state)
      4. Assert: ❄️ badge visible
    Expected Result: Page load reflects frozen state correctly
    Evidence: .sisyphus/evidence/task-7-freeze-persists-reload.png

  Scenario: Unfreeze via UI
    Tool: Playwright
    Preconditions: Course X frozen
    Steps:
      1. Navigate to course list
      2. Find course X, uncheck freeze checkbox
      3. Wait for POST to complete
      4. Assert: checkbox unchecked, ❄️ badge gone
      5. Reload page — assert still unchecked
    Expected Result: Unfreeze persists
    Evidence: .sisyphus/evidence/task-7-unfreeze-ui.png
  ```

  **Commit**: YES (Wave 3, group with Task 6)
  - Message: `feat(courses): add freeze toggle checkbox and frozen badge to course card`
  - Files: `views/` (course card partial)


## Final Verification Wave

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each Must Have: verify implementation exists. For each Must NOT Have: search codebase for forbidden patterns. Check evidence files exist.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Sync Behavior QA** — `unspecified-high`
  1. Create a frozen course in DB (sqlite3 update). 2. Trigger a manual sync. 3. Grep logs for any API call to that course's org_unit_id — expect zero hits. 4. Verify unfreezing resumes calls.
  Save log output to `.sisyphus/evidence/final-qa/sync-freeze-verify.txt`.
  Output: `Freeze skips [PASS/FAIL] | Date ordering [PASS/FAIL] | Course notifications [PASS/FAIL] | VERDICT`

- [ ] F3. **UI QA — Playwright** — `unspecified-high` + `playwright` skill
  Navigate to course card. Find freeze checkbox. Toggle on — verify POST fires and DB updates. Reload — verify checkbox persists. Toggle off — verify DB resets.
  Save screenshot to `.sisyphus/evidence/final-qa/freeze-ui.png`.
  Output: `Toggle ON [PASS/FAIL] | Persists on reload [PASS/FAIL] | Toggle OFF [PASS/FAIL] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `fix(notifications): correct date sorting to prefer StartDate then LastModifiedDate` + `feat(courses): add is_frozen migration` + `fix(notifications): include News type in course-scoped notification queries`
- **Wave 2**: `fix(notifications): skip frozen courses in sync_course_specific_notifications` + `fix(sync): skip all proactive sync loops for frozen courses`
- **Wave 3**: `feat(courses): add freeze/unfreeze endpoint and course card toggle`

---

## Success Criteria

### Verification Commands
```bash
# Confirm is_frozen column exists
sqlite3 brilliant.db ".schema courses" | grep is_frozen

# Confirm a frozen course gets zero sync API calls (check log after manual sync trigger)
grep -i "frozen_course_org_unit_id" log/development.log | tail -20  # expect: 0 matches

# Confirm course-scoped notifications returns both types
curl -s "http://localhost:4567/api/v1/notifications?course_id=COURSE_ID" | ruby -rjson -e 'data=JSON.parse(STDIN.read); puts data.map{|n| n["notification_type"]}.uniq'
# Expected: includes both "Content" and "News"
```

### Final Checklist
- [ ] `is_frozen` column exists, defaults false
- [ ] Frozen courses skipped in ALL sync paths
- [ ] `upsert_notification_batch` uses StartDate → LastModifiedDate → CreatedDate priority
- [ ] Downward date correction does NOT set is_read = false
- [ ] Course-scoped notification query returns News type items
- [ ] Freeze toggle visible on course card, persists, is reversible
