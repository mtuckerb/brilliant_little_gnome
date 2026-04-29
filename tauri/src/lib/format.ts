// Shared numeric formatters. Defaults to 2 decimal places — anywhere we display
// a float coming from the backend (scores, points, weights, GPA, etc.) should
// route through these so trailing IEEE-754 noise doesn't leak into the UI.

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
