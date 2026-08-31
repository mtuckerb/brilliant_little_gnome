import type { Assignment } from "../types";

// Chronological comparator for assignment lists. due_date is TEXT in three
// shapes — Brightspace ISO ("2026-05-04T03:59:00.000Z"), datetime-local edits
// ("2026-05-04T23:59:00"), and Ruby-import rows ("2026-05-04 23:59:00") — so
// string order isn't chronological. Date() parses all three (naive strings as
// local time, which is also how the UI displays them), making this the same
// order the user reads off the page. Rows without a parseable due date sort
// last; ties break by name so the order is stable.
export function compareAssignmentsByDueDate(a: Assignment, b: Assignment): number {
  const ta = dueTime(a.due_date);
  const tb = dueTime(b.due_date);
  if (ta === null && tb === null) return a.name.localeCompare(b.name);
  if (ta === null) return 1;
  if (tb === null) return -1;
  return ta - tb || a.name.localeCompare(b.name);
}

function dueTime(raw: string | null): number | null {
  if (!raw) return null;
  const t = new Date(raw).getTime();
  return Number.isNaN(t) ? null : t;
}
