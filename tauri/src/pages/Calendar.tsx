import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import type { Assignment, Course } from "../types";

// Weekly calendar across all courses. Pulls every course's assignments and
// groups by ISO week starting Sunday.
export default function Calendar() {
  const [data, setData] = useState<{ assignment: Assignment; course: Course }[] | null>(null);

  useEffect(() => {
    (async () => {
      const courses = await api.listCourses();
      const active = courses.filter((c) => c.status !== "dropped" && c.status !== "archived");
      const all = await Promise.all(
        active.map((c) => api.listAssignments(c.org_unit_id).then((as) => as.map((a) => ({ assignment: a, course: c })))),
      );
      setData(all.flat());
    })();
  }, []);

  const grouped = useMemo(() => {
    if (!data) return null;
    const m = new Map<string, { assignment: Assignment; course: Course }[]>();
    for (const row of data) {
      if (!row.assignment.due_date) continue;
      const d = new Date(row.assignment.due_date);
      if (Number.isNaN(d.getTime())) continue;
      // Bucket by week (Sunday). Show only future + last 14 days.
      const now = new Date();
      const cutoff = new Date(now.getTime() - 14 * 86400_000);
      if (d < cutoff) continue;
      const sun = new Date(d);
      sun.setDate(sun.getDate() - sun.getDay());
      sun.setHours(0, 0, 0, 0);
      const key = sun.toISOString().slice(0, 10);
      if (!m.has(key)) m.set(key, []);
      m.get(key)!.push(row);
    }
    return [...m.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [data]);

  if (!grouped) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  return (
    <div>
      <h1 className="title"><i className="fas fa-calendar-days mr-2"></i>Calendar</h1>
      {grouped.length === 0 && <p className="has-text-grey">No upcoming assignments.</p>}
      {grouped.map(([weekKey, rows]) => (
        <div className="box mb-4" key={weekKey}>
          <h2 className="subtitle mb-3">Week of {new Date(weekKey).toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" })}</h2>
          {rows
            .slice()
            .sort((a, b) => (a.assignment.due_date || "").localeCompare(b.assignment.due_date || ""))
            .map(({ assignment: a, course: c }) => (
              <div key={a.id} className="is-flex is-justify-content-space-between py-2" style={{ borderBottom: "1px solid #eee" }}>
                <div>
                  <Link to={`/course/${c.org_unit_id}/assignments`} style={{ color: c.custom_color || undefined }}>
                    {c.code && <strong className="mr-2">{c.code}</strong>}
                    <span className="has-text-weight-medium">{c.name}</span>
                  </Link>
                  <span className="mx-2 has-text-grey-light">·</span>
                  <span>{a.name}</span>
                  {a.completed && <span className="tag is-success is-light ml-2">done</span>}
                  {a.optional && <span className="tag is-light ml-2">optional</span>}
                </div>
                <span className="has-text-grey is-size-7">
                  {a.due_date && new Date(a.due_date).toLocaleString(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" })}
                </span>
              </div>
            ))}
        </div>
      ))}
    </div>
  );
}
