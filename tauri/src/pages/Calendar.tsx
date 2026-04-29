import { useEffect, useMemo, useState, useCallback } from "react";
import { api } from "../api";
import type { Assignment, Course, UserPreferences } from "../types";
import AssignmentRow from "../components/AssignmentRow";

type Row = { assignment: Assignment; course: Course };
const EXIT_MS = 280;

// Weekly calendar across all courses. Pulls every course's assignments and
// groups by ISO week starting Sunday.
export default function Calendar() {
  const [data, setData] = useState<Row[] | null>(null);
  const [showCompleted, setShowCompleted] = useState(false);
  const [leaving, setLeaving] = useState<Set<number>>(new Set());
  const [prefs, setPrefs] = useState<UserPreferences | null>(null);

  const load = useCallback(async () => {
    const [courses, p] = await Promise.all([api.listCourses(), api.getPrefs().catch(() => null)]);
    setPrefs(p);
    const active = courses.filter((c) => c.status !== "dropped" && c.status !== "archived");
    const all = await Promise.all(
      active.map((c) => api.listAssignments(c.org_unit_id, undefined).then((as) => as.map((a) => ({ assignment: a, course: c })))),
    );
    setData(all.flat());
  }, []);

  useEffect(() => { load(); }, [load]);

  async function onToggleComplete(a: Assignment) {
    const willHide = !a.completed && !showCompleted;
    if (willHide) {
      setLeaving((s) => new Set(s).add(a.id));
      await api.toggleAssignmentComplete(a.id);
      setTimeout(() => {
        setLeaving((s) => { const n = new Set(s); n.delete(a.id); return n; });
        load();
      }, EXIT_MS);
    } else {
      await api.toggleAssignmentComplete(a.id);
      load();
    }
  }

  // Empty-day placeholders are a per-user preference toggled in Settings.
  // Default to true while prefs are still loading so the layout doesn't jump.
  const showEmptyDays = prefs?.calendar_show_empty_days ?? true;

  const grouped = useMemo(() => {
    if (!data) return null;
    const m = new Map<string, Row[]>();
    for (const row of data) {
      if (!row.assignment.due_date) continue;
      const d = new Date(row.assignment.due_date);
      if (Number.isNaN(d.getTime())) continue;
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
      <div className="level mb-3">
        <div className="level-left">
          <h1 className="title"><i className="fas fa-calendar-days mr-2"></i>Calendar</h1>
        </div>
        <div className="level-right">
          <button className="button is-small" onClick={() => setShowCompleted((s) => !s)}>
            <span className="icon"><i className={`fas ${showCompleted ? "fa-eye-slash" : "fa-eye"}`}></i></span>
            <span>{showCompleted ? "Hide completed" : "Show completed"}</span>
          </button>
        </div>
      </div>
      {grouped.length === 0 && <p className="has-text-grey">No upcoming assignments.</p>}
      {grouped.map(([weekKey, rows]) => {
        const weekStart = new Date(weekKey);
        const sortedRows = rows
          .slice()
          .filter((r) => !r.assignment.completed || showCompleted || leaving.has(r.assignment.id))
          .sort((a, b) => (a.assignment.due_date || "").localeCompare(b.assignment.due_date || ""));

        // Per-day buckets so we can render empty-day placeholders.
        const byDay = new Map<string, Row[]>();
        for (const r of sortedRows) {
          const d = new Date(r.assignment.due_date!);
          d.setHours(0, 0, 0, 0);
          const k = d.toISOString().slice(0, 10);
          if (!byDay.has(k)) byDay.set(k, []);
          byDay.get(k)!.push(r);
        }

        return (
          <div className="box mb-4" key={weekKey}>
            <h2 className="subtitle mb-3">Week of {weekStart.toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" })}</h2>
            {Array.from({ length: 7 }).map((_, i) => {
              const day = new Date(weekStart);
              day.setDate(day.getDate() + i);
              const k = day.toISOString().slice(0, 10);
              const dayRows = byDay.get(k) || [];
              if (dayRows.length === 0) {
                if (!showEmptyDays) return null;
                return (
                  <div className="calendar-empty-day" key={k} title={day.toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric" })} />
                );
              }
              return (
                <div key={k}>
                  {dayRows.map(({ assignment: a, course: c }) => (
                    <AssignmentRow
                      key={a.id}
                      assignment={a}
                      course={c}
                      showCourse
                      leaving={leaving.has(a.id)}
                      onToggleComplete={onToggleComplete}
                    />
                  ))}
                </div>
              );
            })}
          </div>
        );
      })}
    </div>
  );
}
