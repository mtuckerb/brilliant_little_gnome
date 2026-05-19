// Parse + sort + normalize Brightspace's per-course semester strings.
//
// Brightspace ships values in a couple of shapes — "2025 Spring", "Spring
// 2025", occasionally "2026SP", "Summer Session 2025", "2025-2026 AY". We
// extract a year + a season ordinal and let the caller display whatever
// normalized form looks best.
//
// Sort order is **descending** by (year, season) — so a Dashboard reads
// "current semester first" without the user having to scroll.

const SEASON_ORDER: Record<string, number> = {
  winter: 0,
  spring: 1,
  summer: 2,
  fall: 3,
  autumn: 3,
  session: 2,  // Summer Session — group with Summer
  quarter: 2,
};

const SEASON_RE = /(winter|spring|summer|fall|autumn|session|quarter)/i;
const YEAR_RE = /(20\d{2})/;
const SHORT_RE = /(20\d{2})\s*(sp|su|fa|wi)\b/i;
const SHORT_SEASON: Record<string, string> = { sp: "spring", su: "summer", fa: "fall", wi: "winter" };

export interface ParsedSemester {
  year: number | null;
  seasonOrder: number | null;
  // Normalized display form. "2025 Spring" for parseable strings, original
  // string otherwise (so unknown formats round-trip without being mangled).
  normalized: string;
}

export function parseSemester(raw: string | null | undefined): ParsedSemester {
  if (!raw) {
    return { year: null, seasonOrder: null, normalized: "Other" };
  }
  const s = raw.trim();
  if (s === "") {
    return { year: null, seasonOrder: null, normalized: "Other" };
  }

  // Short form first ("2026SP", "2025 FA").
  const short = SHORT_RE.exec(s);
  if (short) {
    const year = Number(short[1]);
    const season = SHORT_SEASON[short[2].toLowerCase()];
    const order = SEASON_ORDER[season] ?? null;
    const titled = season.charAt(0).toUpperCase() + season.slice(1);
    return { year, seasonOrder: order, normalized: `${year} ${titled}` };
  }

  const seasonMatch = SEASON_RE.exec(s);
  const yearMatch = YEAR_RE.exec(s);
  const year = yearMatch ? Number(yearMatch[1]) : null;
  const seasonKey = seasonMatch ? seasonMatch[1].toLowerCase() : null;
  const seasonOrder = seasonKey ? SEASON_ORDER[seasonKey] ?? null : null;

  if (year !== null && seasonKey !== null) {
    const titled = seasonKey.charAt(0).toUpperCase() + seasonKey.slice(1);
    return { year, seasonOrder, normalized: `${year} ${titled}` };
  }
  // Couldn't parse fully — keep the original so we don't lose information.
  return { year, seasonOrder, normalized: s };
}

/// Compare for sorting: most recent semester first. Unparseable values sort
/// to the end (after every dated semester).
export function compareSemestersDesc(a: ParsedSemester, b: ParsedSemester): number {
  const aYear = a.year ?? -Infinity;
  const bYear = b.year ?? -Infinity;
  if (aYear !== bYear) return bYear - aYear;
  const aSeason = a.seasonOrder ?? -Infinity;
  const bSeason = b.seasonOrder ?? -Infinity;
  if (aSeason !== bSeason) return bSeason - aSeason;
  return a.normalized.localeCompare(b.normalized);
}
