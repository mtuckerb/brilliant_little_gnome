import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, onAppEvent } from "../api";
import { courseLabel, displayCourseCode, displayCourseName, displayCourseSemester, stripLeadingCode, type Course } from "../types";
import { getLastCoursePath } from "../lib/courseLastTab";
import { compareSemestersDesc, parseSemester } from "../lib/semester";
import { useIsMobile } from "../hooks/useIsMobile";
import DueSoon from "../components/DueSoon";

export default function Dashboard() {
  const [courses, setCourses] = useState<Course[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const isMobile = useIsMobile();

  const refresh = useCallback(() => {
    return api
      .listCourses()
      .then((c) =>
        setCourses(c.filter((x) => x.status !== "withdrawn" && x.status !== "dropped_fail")),
      )
      .catch((e) => setErr(String(e?.message ?? e)));
  }, []);

  useEffect(() => {
    refresh();
    // Re-fetch on sync state transitions: the very first run lands here
    // before the background sync has populated `courses`, so without
    // this the user sees an empty Dashboard until they navigate away
    // and back. We refresh on every status change but only actually
    // re-render when the row count changes.
    let prev: string | null = null;
    const unlisten = onAppEvent((e) => {
      if (e.kind === "sync_status_changed") {
        const next = e.status.status;
        setSyncing(next === "syncing");
        if (prev === "syncing" && next !== "syncing") refresh();
        prev = next;
      }
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [refresh]);

  if (err) return <div className="notification is-danger">{err}</div>;
  if (courses === null) {
    return (
      <div className="has-text-centered py-6">
        <span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span>
      </div>
    );
  }

  if (courses.length === 0) {
    return (
      <div className="has-text-centered py-6">
        <h1 className="title is-3">Dashboard</h1>
        <div className="box mx-auto" style={{ maxWidth: 480 }}>
          {syncing ? (
            <>
              <p className="mb-3">
                <span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-2x"></i></span>
              </p>
              <p className="has-text-grey">Pulling your courses from Brightspace…</p>
              <p className="is-size-7 has-text-grey-light mt-2">First sync can take a minute.</p>
            </>
          ) : (
            <>
              <p className="mb-3">
                <span className="icon is-large has-text-grey-light"><i className="fas fa-folder-open fa-2x"></i></span>
              </p>
              <p className="has-text-grey">No courses yet.</p>
              <button className="button is-primary mt-4" onClick={() => api.syncAll(true)}>
                <span className="icon"><i className="fas fa-sync"></i></span>
                <span>Sync now</span>
              </button>
            </>
          )}
        </div>
      </div>
    );
  }

  async function togglePin(c: Course) {
    const next = !c.is_pinned;
    // Optimistic: flip the icon immediately, then re-fetch to re-sort
    // (pinned courses float to the top of their semester).
    setCourses((prev) =>
      prev ? prev.map((x) => (x.org_unit_id === c.org_unit_id ? { ...x, is_pinned: next } : x)) : prev,
    );
    try {
      await api.setCoursePinned(c.org_unit_id, next);
    } finally {
      refresh();
    }
  }

  if (isMobile) {
    return <MobileDashboard courses={courses} onTogglePin={togglePin} />;
  }

  // Group courses under the normalized semester label so "Spring 2025" and
  // "2025 Spring" merge under one heading. Then sort the headings most-
  // recent-first using the parsed (year, seasonOrder). Prefer custom_semester
  // when set (the cross-semester drag writes it).
  const bySemester: Record<string, Course[]> = {};
  const parsedFor: Record<string, ReturnType<typeof parseSemester>> = {};
  for (const c of courses) {
    const parsed = parseSemester(displayCourseSemester(c));
    parsedFor[parsed.normalized] = parsed;
    (bySemester[parsed.normalized] ??= []).push(c);
  }
  const orderedSemesters = Object.keys(bySemester).sort((a, b) =>
    compareSemestersDesc(parsedFor[a], parsedFor[b]),
  );

  // Compute the flat ordered list across all semesters for use by the
  // drag-reorder handler. Same iteration order as the rendered groups.
  function flatOrder(list: Course[]): Course[] {
    const groups: Record<string, Course[]> = {};
    const ps: Record<string, ReturnType<typeof parseSemester>> = {};
    for (const c of list) {
      const p = parseSemester(displayCourseSemester(c));
      ps[p.normalized] = p;
      (groups[p.normalized] ??= []).push(c);
    }
    const keys = Object.keys(groups).sort((a, b) => compareSemestersDesc(ps[a], ps[b]));
    return keys.flatMap((k) => groups[k]);
  }

  async function onDropOnCourse(target: Course, e: React.DragEvent<HTMLTableRowElement>) {
    e.preventDefault();
    if (!courses) return;
    const dragId = e.dataTransfer.getData("text/course-id");
    if (!dragId || dragId === target.org_unit_id) return;
    const altHeld = e.altKey;
    const dragged = courses.find((c) => c.org_unit_id === dragId);
    if (!dragged) return;
    const targetSemester = displayCourseSemester(target);
    const dragSemester = displayCourseSemester(dragged);

    // Cross-semester drop requires alt. Otherwise silently ignore — UX cue
    // is the lack of drop highlight (added separately).
    const sameSemester = (targetSemester ?? "") === (dragSemester ?? "");
    if (!sameSemester && !altHeld) return;

    // Build the new course list with the dragged item moved.
    let working = courses.map((c) =>
      c.org_unit_id === dragId && !sameSemester && altHeld
        ? { ...c, custom_semester: targetSemester ?? null }
        : c,
    );
    // Remove dragged from its current position, then insert before target.
    working = working.filter((c) => c.org_unit_id !== dragId);
    const targetIx = working.findIndex((c) => c.org_unit_id === target.org_unit_id);
    if (targetIx < 0) return;
    const insertIx = targetIx;
    const movedCopy = courses.find((c) => c.org_unit_id === dragId)!;
    const finalDragged =
      !sameSemester && altHeld ? { ...movedCopy, custom_semester: targetSemester ?? null } : movedCopy;
    working.splice(insertIx, 0, finalDragged);

    // Optimistic UI.
    setCourses(working);

    try {
      if (!sameSemester && altHeld) {
        await api.updateCourseSemester(dragId, targetSemester);
      }
      // reorder_courses sorts every course by its position in the list, so
      // we send the full flat order (across all semesters) to keep the
      // sort_order globally consistent.
      const flat = flatOrder(working).map((c) => c.org_unit_id);
      await api.reorderCourses(flat);
    } catch {
      refresh();
    }
  }

  return (
    <div>
      <h1 className="title is-3">Dashboard</h1>
      <div className="level mb-4">
        <div className="level-left">
          <p className="has-text-grey">{courses.length} active course{courses.length === 1 ? "" : "s"}</p>
        </div>
        <div className="level-right">
          <button className="button is-small" onClick={() => api.syncAll(true)}>
            <span className="icon"><i className="fas fa-sync"></i></span>
            <span>Sync</span>
          </button>
        </div>
      </div>

      <DueSoon />

      {orderedSemesters.map((sem) => {
        const list = bySemester[sem];
        return (
        <div key={sem} className="box">
          <h2 className="title is-5 mb-3">{sem}</h2>
          <table className="table is-fullwidth is-hoverable">
            <tbody>
              {list.map((c) => (
                <tr
                  key={c.org_unit_id}
                  className="course-row"
                  draggable
                  onDragStart={(e) => {
                    e.dataTransfer.setData("text/course-id", c.org_unit_id);
                    e.dataTransfer.effectAllowed = "move";
                  }}
                  onDragOver={(e) => {
                    const dragId = e.dataTransfer.types.includes("text/course-id") ? "(any)" : null;
                    if (!dragId) return;
                    // Allow drop only same-semester, OR cross-semester with alt held.
                    // (DataTransfer.getData isn't available in dragover, so we
                    // optimistically accept and validate on drop.)
                    e.preventDefault();
                    e.dataTransfer.dropEffect = e.altKey ? "copy" : "move";
                  }}
                  onDrop={(e) => onDropOnCourse(c, e)}
                  style={{ cursor: "grab" }}
                  title="Drag to reorder. Hold ⌥/Alt while dropping to move across semesters."
                >
                  <td>
                    <button
                      type="button"
                      className="button is-white is-small px-1 mr-1"
                      title={c.is_pinned ? "Unpin course" : "Pin course"}
                      aria-label={c.is_pinned ? "Unpin course" : "Pin course"}
                      onClick={(e) => { e.preventDefault(); e.stopPropagation(); togglePin(c); }}
                      onDragStart={(e) => e.preventDefault()}
                    >
                      <span className="icon is-small">
                        <i
                          className={`fas fa-thumbtack ${c.is_pinned ? "has-text-warning" : "has-text-grey-light"}`}
                          style={{ opacity: c.is_pinned ? 1 : 0.55, transform: c.is_pinned ? "none" : "rotate(40deg)" }}
                        ></i>
                      </span>
                    </button>
                    <Link
                      to={getLastCoursePath(c.org_unit_id)}
                      className="has-text-weight-bold"
                      style={{ color: c.custom_color || undefined }}
                    >
                      {courseLabel(c)}
                    </Link>
                  </td>
                  <td className="has-text-right">
                    <Link to={`/course/${c.org_unit_id}/grades`} className="button is-small is-light mr-2">
                      <span className="icon"><i className="fas fa-poll"></i></span><span>Grades</span>
                    </Link>
                    <Link to={`/course/${c.org_unit_id}/assignments`} className="button is-small is-light">
                      <span className="icon"><i className="fas fa-tasks"></i></span><span>Assignments</span>
                    </Link>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        );
      })}
    </div>
  );
}

interface Stat { value: string; label: string }

function MobileDashboard({ courses, onTogglePin }: { courses: Course[]; onTogglePin: (c: Course) => void }) {
  // Stats are derivable from the courses list alone — no extra fetches on
  // load. Grade roll-up + due-date counts can move into here once we
  // batch-load grades summary per course; for v1 the simple counts read
  // honest and don't slow the dashboard down.
  const semesterCount = new Set(courses.map((c) => parseSemester(displayCourseSemester(c)).normalized)).size;
  const pinnedCount = courses.filter((c) => c.is_pinned).length;
  const stats: Stat[] = [
    { value: String(courses.length), label: `Course${courses.length === 1 ? "" : "s"}` },
    { value: String(semesterCount), label: `Semester${semesterCount === 1 ? "" : "s"}` },
    { value: String(pinnedCount), label: "Pinned" },
  ];

  // Group + sort for the semester chips. Chips select a single semester
  // to filter the list; "All" shows everything.
  const groups: Record<string, Course[]> = {};
  const parsedFor: Record<string, ReturnType<typeof parseSemester>> = {};
  for (const c of courses) {
    const p = parseSemester(displayCourseSemester(c));
    parsedFor[p.normalized] = p;
    (groups[p.normalized] ??= []).push(c);
  }
  const orderedSemesters = Object.keys(groups).sort((a, b) =>
    compareSemestersDesc(parsedFor[a], parsedFor[b]),
  );

  const [activeSemester, setActiveSemester] = useState<string>(orderedSemesters[0] ?? "");
  // Default to the newest semester when the data lands.
  useEffect(() => {
    if (!activeSemester && orderedSemesters.length > 0) {
      setActiveSemester(orderedSemesters[0]);
    }
  }, [orderedSemesters, activeSemester]);

  const visible = activeSemester ? (groups[activeSemester] ?? []) : courses;

  return (
    <div>
      {/* Pencil frame: appBar / appTitleWrap (HDUr0). */}
      <div className="mb-4">
        <h1 className="pencil-appTitle title is-4 mb-1">Student Courses</h1>
        <p className="pencil-appSub">Course updates and due dates</p>
      </div>

      {/* Pencil frame: statsRow (HDUr0). */}
      <div className="pencil-statsRow mb-4">
        {stats.map((s) => (
          <div key={s.label} className="pencil-stat">
            <div className="pencil-stat-value">{s.value}</div>
            <div className="pencil-stat-label">{s.label}</div>
          </div>
        ))}
      </div>

      {/* Pencil frame: semesterCoursesPanel (HDUr0). */}
      <div className="pencil-semesterCoursesPanel">
        <div className="is-flex is-align-items-center is-justify-content-space-between mb-3">
          <h2 className="title is-6 mb-0">
            {activeSemester ? activeSemester : "All courses"}
          </h2>
          <span className="is-size-7 has-text-grey">{visible.length} course{visible.length === 1 ? "" : "s"}</span>
        </div>

        {orderedSemesters.length > 1 && (
          <div
            className="is-flex mb-3"
            style={{ gap: 6, overflowX: "auto", paddingBottom: 4 }}
          >
            {orderedSemesters.map((sem) => (
              <button
                key={sem}
                type="button"
                onClick={() => setActiveSemester(sem)}
                className={`pencil-semester-chip${activeSemester === sem ? " is-active" : ""}`}
                style={{ flex: "0 0 auto" }}
              >
                {sem}
              </button>
            ))}
          </div>
        )}

        {/* Pencil frame: course-rows / course-row-N (c3DdR). */}
        <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
          {visible.map((c) => (
            <li key={c.org_unit_id}>
              <Link
                to={getLastCoursePath(c.org_unit_id)}
                className="pencil-course-row"
              >
                <span
                  style={{
                    width: 8,
                    height: 44,
                    flex: "0 0 auto",
                    borderRadius: "var(--pencil-radius-xs)",
                    background: c.custom_color || "var(--pencil-text-secondary)",
                  }}
                />
                <div style={{ flex: "1 1 auto", minWidth: 0 }}>
                  <div className="pencil-course-row-title">
                    {stripLeadingCode(displayCourseName(c))}
                  </div>
                  <div className="pencil-course-row-meta">
                    {displayCourseCode(c) ?? ""}
                  </div>
                </div>
                <button
                  type="button"
                  className="button is-white is-small px-2"
                  title={c.is_pinned ? "Unpin course" : "Pin course"}
                  aria-label={c.is_pinned ? "Unpin course" : "Pin course"}
                  onClick={(e) => { e.preventDefault(); e.stopPropagation(); onTogglePin(c); }}
                >
                  <span className="icon">
                    <i
                      className={`fas fa-thumbtack ${c.is_pinned ? "has-text-warning" : "has-text-grey-light"}`}
                      style={{ opacity: c.is_pinned ? 1 : 0.55, transform: c.is_pinned ? "none" : "rotate(40deg)" }}
                    ></i>
                  </span>
                </button>
                <span className="icon" style={{ color: "var(--pencil-text-secondary)" }}>
                  <i className="fas fa-chevron-right"></i>
                </span>
              </Link>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
