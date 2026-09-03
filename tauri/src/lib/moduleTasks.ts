import type { ContentModule } from "../types";

const MONTHS: Record<string, number> = {
  jan: 0,
  january: 0,
  feb: 1,
  february: 1,
  mar: 2,
  march: 2,
  apr: 3,
  april: 3,
  may: 4,
  jun: 5,
  june: 5,
  jul: 6,
  july: 6,
  aug: 7,
  august: 7,
  sep: 8,
  sept: 8,
  september: 8,
  oct: 9,
  october: 9,
  nov: 10,
  november: 10,
  dec: 11,
  december: 11,
};

function validLocalDate(year: number, month: number, day: number): Date | null {
  const date = new Date(year, month, day, 12, 0, 0, 0);
  return date.getFullYear() === year && date.getMonth() === month && date.getDate() === day
    ? date
    : null;
}

function normalizedYear(value: string | undefined, fallback: number): number {
  if (!value) return fallback;
  const parsed = Number(value);
  if (value.length === 2) return parsed >= 70 ? 1900 + parsed : 2000 + parsed;
  return parsed;
}

/**
 * Read the calendar anchor from a dated module heading. Supported examples:
 * "Week 1: September 2, 2026", "Week 3 - 1/19/26", and
 * "Week ending 7 September".
 */
export function dateFromModuleTitle(title: string, fallbackYear = new Date().getFullYear()): Date | null {
  const iso = /\b(20\d{2})-(\d{1,2})-(\d{1,2})\b/.exec(title);
  if (iso) return validLocalDate(Number(iso[1]), Number(iso[2]) - 1, Number(iso[3]));

  const numeric = /\b(\d{1,2})[/-](\d{1,2})(?:[/-](\d{2}|\d{4}))?\b/.exec(title);
  if (numeric) {
    return validLocalDate(
      normalizedYear(numeric[3], fallbackYear),
      Number(numeric[1]) - 1,
      Number(numeric[2]),
    );
  }

  const monthFirst = new RegExp(
    `\\b(${Object.keys(MONTHS).join("|")})\\.?\\s*,?\\s*(\\d{1,2})(?:\\s*,?\\s*(20\\d{2}|\\d{2}))?\\b`,
    "i",
  ).exec(title);
  if (monthFirst) {
    return validLocalDate(
      normalizedYear(monthFirst[3], fallbackYear),
      MONTHS[monthFirst[1].toLowerCase()],
      Number(monthFirst[2]),
    );
  }

  const dayFirst = new RegExp(
    `\\b(\\d{1,2})\\s+(${Object.keys(MONTHS).join("|")})\\.?(?:\\s*,?\\s*(20\\d{2}|\\d{2}))?\\b`,
    "i",
  ).exec(title);
  if (dayFirst) {
    return validLocalDate(
      normalizedYear(dayFirst[3], fallbackYear),
      MONTHS[dayFirst[2].toLowerCase()],
      Number(dayFirst[1]),
    );
  }

  return null;
}

function datetimeLocalValue(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T23:59`;
}

/**
 * Find the closest dated module in a row's ancestry and return the configured
 * last class day in that module's week as a datetime-local input value.
 */
export function moduleTaskDue(
  moduleId: string,
  modules: readonly ContentModule[],
  endOfWeekDay: number | null,
  fallbackYear = new Date().getFullYear(),
): string | undefined {
  const byId = new Map(modules.map((module) => [module.brightspace_id, module]));
  const seen = new Set<string>();
  let current = byId.get(moduleId);

  while (current && !seen.has(current.brightspace_id)) {
    seen.add(current.brightspace_id);
    const anchor = dateFromModuleTitle(current.title, fallbackYear);
    if (anchor) {
      const targetDay = endOfWeekDay !== null && endOfWeekDay >= 0 && endOfWeekDay <= 6
        ? endOfWeekDay
        : 0;
      anchor.setDate(anchor.getDate() + ((targetDay - anchor.getDay() + 7) % 7));
      return datetimeLocalValue(anchor);
    }
    current = current.parent_id ? byId.get(current.parent_id) : undefined;
  }

  return undefined;
}
