import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import type { Course } from "../types";

export default function Dashboard() {
  const [courses, setCourses] = useState<Course[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    api.listCourses()
      .then((c) => setCourses(c.filter((x) => x.status !== "withdrawn" && x.status !== "dropped_fail")))
      .catch((e) => setErr(String(e?.message ?? e)));
  }, []);

  if (err) return <div className="notification is-danger">{err}</div>;
  if (courses === null) {
    return (
      <div className="has-text-centered py-6">
        <span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span>
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
                      {c.code && <span className="mr-2">{c.code}</span>}
                      <span className="has-text-weight-normal">{c.name}</span>
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
