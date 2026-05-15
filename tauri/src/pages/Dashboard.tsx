import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, onAppEvent } from "../api";
import { displayCourseName, type Course } from "../types";

export default function Dashboard() {
  const [courses, setCourses] = useState<Course[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);

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
        <h1 className="title is-3"><i className="fas fa-graduation-cap mr-2"></i>Dashboard</h1>
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

  const bySemester: Record<string, Course[]> = {};
  for (const c of courses) {
    const k = c.semester ?? "Other";
    (bySemester[k] ??= []).push(c);
  }

  return (
    <div>
      <h1 className="title is-3"><i className="fas fa-graduation-cap mr-2"></i>Dashboard</h1>
      <div className="level mb-4">
        <div className="level-left">
          <p className="has-text-grey">{courses.length} active course{courses.length === 1 ? "" : "s"}</p>
        </div>
        <div className="level-right">
          <button className="button is-small" onClick={() => api.syncAll(false)}>
            <span className="icon"><i className="fas fa-sync"></i></span>
            <span>Sync</span>
          </button>
        </div>
      </div>

      {Object.entries(bySemester).map(([sem, list]) => (
        <div key={sem} className="box">
          <h2 className="title is-5 mb-3">{sem}</h2>
          <table className="table is-fullwidth is-hoverable">
            <tbody>
              {list.map((c) => (
                <tr key={c.org_unit_id} className="course-row">
                  <td>
                    <Link
                      to={`/course/${c.org_unit_id}`}
                      className="has-text-weight-bold"
                      style={{ color: c.custom_color || undefined }}
                    >
                      {c.is_pinned && <i className="fas fa-thumbtack has-text-warning mr-2"></i>}
                      {c.code ? `${c.code} - ${displayCourseName(c)}` : displayCourseName(c)}
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
      ))}
    </div>
  );
}
