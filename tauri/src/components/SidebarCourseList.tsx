import { useCallback, useEffect, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { api, onAppEvent } from "../api";
import { type Course, displayCourseCode, displayCourseName, stripLeadingCode } from "../types";
import { compareSemestersDesc, parseSemester } from "../lib/semester";
import { getLastCoursePath, parseCoursePath } from "../lib/courseLastTab";

// Dark left-rail course list, grouped by semester (newest first). Each row
// shows a vertical color swatch (the course's custom_color), the code, and
// the name. Letter-initial icons were rejected in the design review — the
// color rail carries the same affordance with less noise.
//
// "Active" highlight matches whichever course the user is currently inside
// (/course/:id/*).

interface Props {
  filter: string;
}

interface Group {
  label: string;
  courses: Course[];
}

export default function SidebarCourseList({ filter }: Props) {
  const [courses, setCourses] = useState<Course[]>([]);
  const location = useLocation();
  const activeCourseId = parseCoursePath(location.pathname)?.courseId ?? null;

  const refresh = useCallback(() => {
    api
      .listCourses()
      .then((c) => setCourses(c.filter((x) => x.status !== "withdrawn" && x.status !== "dropped_fail" && x.status !== "dropped" && x.status !== "archived")))
      .catch(() => setCourses([]));
  }, []);

  useEffect(() => {
    refresh();
    let prev: string | null = null;
    const unlisten = onAppEvent((e) => {
      if (e.kind === "sync_status_changed") {
        const next = e.status.status;
        if (prev === "syncing" && next !== "syncing") refresh();
        prev = next;
      }
    });
    return () => { unlisten.then((fn) => fn()).catch(() => {}); };
  }, [refresh]);

  // Group + sort + (optional) text filter.
  const needle = filter.trim().toLowerCase();
  const filtered = needle
    ? courses.filter((c) => {
        const code = (displayCourseCode(c) ?? "").toLowerCase();
        const name = displayCourseName(c).toLowerCase();
        return code.includes(needle) || name.includes(needle);
      })
    : courses;
  const groups: Group[] = [];
  const buckets: Record<string, Group> = {};
  const parsed: Record<string, ReturnType<typeof parseSemester>> = {};
  for (const c of filtered) {
    const p = parseSemester(c.semester);
    parsed[p.normalized] = p;
    if (!buckets[p.normalized]) {
      const g: Group = { label: p.normalized, courses: [] };
      buckets[p.normalized] = g;
      groups.push(g);
    }
    buckets[p.normalized].courses.push(c);
  }
  groups.sort((a, b) => compareSemestersDesc(parsed[a.label], parsed[b.label]));

  return (
    <div style={{ overflowY: "auto", flex: "1 1 auto", paddingTop: 8 }}>
      {groups.length === 0 && (
        <div className="px-3 py-3 is-size-7" style={{ color: "var(--pencil-text-on-dark-dim)" }}>
          {filter ? "No matches" : "No courses yet."}
        </div>
      )}
      {groups.map((g) => (
        <div key={g.label} style={{ marginBottom: 12 }}>
          <div className="pencil-semHdr is-size-7">{g.label}</div>
          {g.courses.map((c) => {
            const isActive = c.org_unit_id === activeCourseId;
            const swatch = c.custom_color || "#5A6573";
            return (
              <Link
                key={c.org_unit_id}
                to={getLastCoursePath(c.org_unit_id)}
                className={`course-sidebar-row pencil-sidebar-row${isActive ? " is-active" : ""}`}
                style={{
                  borderLeft: isActive ? `3px solid ${swatch}` : "3px solid transparent",
                }}
              >
                <span
                  style={{
                    width: 8,
                    height: 32,
                    borderRadius: 3,
                    background: swatch,
                    flex: "0 0 auto",
                    opacity: isActive ? 1 : 0.85,
                  }}
                />
                <div style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0 }}>
                  <span
                    style={{
                      fontSize: 11,
                      fontWeight: 700,
                      color: isActive
                        ? "var(--pencil-text-on-dark)"
                        : "var(--pencil-text-on-dark-dim)",
                      letterSpacing: 0.3,
                    }}
                  >
                    {displayCourseCode(c) ?? "—"}
                  </span>
                  <span
                    style={{
                      fontSize: 11,
                      color: isActive
                        ? "var(--pencil-text-on-dark-dim)"
                        : "var(--pencil-text-on-dark-faint)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {stripLeadingCode(displayCourseName(c))}
                  </span>
                </div>
              </Link>
            );
          })}
        </div>
      ))}
    </div>
  );
}
