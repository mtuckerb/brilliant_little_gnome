// Remembers the last sub-page the user visited for each course so the
// Dashboard course link drops them back where they actually work — Modules,
// Grades, etc — instead of always landing on the Overview.
//
// State lives in localStorage so it survives app restarts. We only persist
// the sub-path segment (e.g. "content", "grades", or "" for the overview)
// rather than the full URL so renamed routes stay correct without migration.

const STORAGE_KEY = "brilliant.lastCourseTab.v1";

// Path segments accepted as a valid sub-page. Anything else (including a
// stray detail-page id like /content/12345) falls through to its parent.
const VALID_SUBPATHS = new Set(["", "grades", "assignments", "content", "discussions", "announcements", "search"]);

interface LastTabMap { [courseId: string]: string }

function load(): LastTabMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    return parsed as LastTabMap;
  } catch {
    return {};
  }
}

function save(map: LastTabMap) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
  } catch {
    // Quota / private mode — non-fatal.
  }
}

/// Returns the URL the Dashboard (or any other entry point) should send the
/// user to for this course. Defaults to `/course/:id` (Overview) when no tab
/// has been recorded yet.
export function getLastCoursePath(courseId: string): string {
  const map = load();
  const sub = map[courseId];
  if (!sub) return `/course/${courseId}`;
  return `/course/${courseId}/${sub}`;
}

/// Record a visit to a course sub-page. Caller passes the path segment
/// directly after the course id (e.g. "content" or "" for overview). Detail
/// pages (`/content/:moduleId`) record their parent so the next click lands
/// back on the listing rather than a specific module.
export function recordCourseVisit(courseId: string, subPath: string) {
  if (!VALID_SUBPATHS.has(subPath)) return;
  const map = load();
  if (map[courseId] === subPath) return;
  map[courseId] = subPath;
  save(map);
}

/// Parse a URL pathname and, if it's a course route, return (courseId, sub).
/// `sub` is the empty string for the overview, the segment for top-level
/// sub-pages, or the parent segment for known detail pages.
export function parseCoursePath(pathname: string): { courseId: string; subPath: string } | null {
  // Strip leading slash and split.
  const parts = pathname.replace(/^\/+/, "").split("/");
  if (parts[0] !== "course" || !parts[1]) return null;
  const courseId = parts[1];
  const sub = parts[2] ?? "";
  if (sub === "") return { courseId, subPath: "" };
  if (VALID_SUBPATHS.has(sub)) return { courseId, subPath: sub };
  return null;
}
