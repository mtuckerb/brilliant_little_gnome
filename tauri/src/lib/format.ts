// Shared display formatters. Defaults to 2 decimal places for numbers —
// anywhere we display backend values should route through these so formatting
// stays consistent across pages.

/**
 * Format a number to `places` decimals, dropping trailing zeros and the
 * decimal point if not needed (so 95 → "95", 95.5 → "95.5", 95.499 → "95.5"
 * with places=2). Returns `dash` for null/undefined/NaN.
 */
export function fmtNum(
  n: number | null | undefined,
  places = 2,
  dash = "—",
): string {
  if (n === null || n === undefined || Number.isNaN(n)) return dash;
  // toFixed produces trailing zeros; parseFloat → toString strips them while
  // preserving the rounded value.
  const rounded = parseFloat(n.toFixed(places));
  return rounded.toString();
}

/** Same as fmtNum but appends "%". */
export function fmtPct(
  n: number | null | undefined,
  places = 2,
  dash = "—",
): string {
  if (n === null || n === undefined || Number.isNaN(n)) return dash;
  return `${fmtNum(n, places, dash)}%`;
}

/**
 * Format assignment due dates with an explicit calendar date so students never
 * have to infer the year from a partial weekday/month label. Brightspace due
 * dates arrive as ISO-like strings; parsing preserves the existing timezone
 * behavior used by the UI while centralizing the visible date shape.
 */
export function fmtAssignmentDueDate(
  dueDate: string | null | undefined,
  empty = "No due date",
): string {
  if (!dueDate) return empty;

  const date = new Date(dueDate);
  if (Number.isNaN(date.getTime())) return dueDate;

  const year = String(date.getFullYear());
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  const time = date.toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });

  return `${year}-${month}-${day} ${time}`;
}
