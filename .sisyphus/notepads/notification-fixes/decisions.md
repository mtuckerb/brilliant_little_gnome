# Decisions - Notification Fixes

## is_frozen Column Implementation (2026-02-23)

### Decision: Direct SQLite Manipulation for Migration
- **Context**: Ruby version mismatch (system Ruby 2.6 vs required 3.1+) prevented running `bundle install` and `ruby migrate.rb`
- **Choice**: Applied migration directly via `sqlite3` CLI instead of Rails migration runner
- **Rationale**: 
  - Migration file created with proper guard clause (`unless column_exists?`)
  - Direct SQLite execution achieves same result: `is_frozen INTEGER DEFAULT 0 NOT NULL`
  - Avoids dependency on portable Ruby distribution or version manager
  - Migration file remains in place for future Rails environments

### Implementation Details
- **Migration File**: `db/migrate/20260223164726_add_is_frozen_to_courses.rb`
  - Uses standard Rails migration pattern with `unless column_exists?` guard
  - Defines boolean column with `default: false, null: false`
  
- **Model Method**: Added `is_frozen?` to `Course` model (line 31-33)
  - Returns explicit boolean: `is_frozen == true`
  - Follows existing pattern of `dropped?` and `fail_on_drop?` methods

### Verification
- Schema check: `PRAGMA table_info(courses)` confirms column 19: `is_frozen|INTEGER|1|0|0`
- Column properties: NOT NULL, DEFAULT 0 (false)
- Model method: Defined and ready for use
- All requirements met per task specification
