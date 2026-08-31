import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, onAppEvent } from "../api";
import type { Assignment, Course } from "../types";
import { compareAssignmentsByDueDate } from "../lib/assignments";
import { fmtAssignmentDueDate } from "../lib/format";

// Pencil frame: "Due soon" (Desktop Vision > rightCol, frame id RLKQX).
//
// A compact card listing the next few upcoming, not-yet-completed
// assignments across every active course. It does NOT introduce a new data
// flow: it reads the same already-synced assignment rows the Calendar page
// streams (api.listCourses + api.listAssignments), just surfaced as the
// dashboard widget the Pencil design calls for. No new backend command.
//
// Structure mirrors the .pen frame: a `dueHdr` row (clock + "Due soon")
// followed by `due<n>` rows, each a `.pencil-due-row` with a title line and
// a relative-time meta line (warn color when due within 48h, per the
// `#D4A437` swatch on `due1` in the design).

interface Item {
  assignment: Assignment;
  course: Course;
}

const LOOKAHEAD_DAYS = 14;
const MAX_ITEMS = 5;
const SOON_MS = 48 * 3_600_000;


export default function DueSoon() {
  const [items, setItems] = useState<Item[] | null>(null);

  const refresh = useCallback(async () => {
    try {
      const courses = await api.listCourses();
      const active = courses.filter(
        (c) => c.status !== "withdrawn" && c.status !== "dropped_fail" && c.status !== "dropped" && c.status !== "archived",
      );
      const perCourse = await Promise.all(
        active.map((c) =>
          api
            .listAssignments(c.org_unit_id, undefined)
            .then((as) => as.map((a) => ({ assignment: a, course: c })))
            .catch(() => [] as Item[]),
        ),
      );
      const now = Date.now();
      const horizon = now + LOOKAHEAD_DAYS * 86_400_000;
      const upcoming = perCourse
        .flat()
        .filter(({ assignment: a }) => {
          if (a.completed || !a.due_date) return false;
          const t = new Date(a.due_date).getTime();
          if (Number.isNaN(t)) return false;
          return t >= now && t <= horizon;
        })
        .sort((x, y) => compareAssignmentsByDueDate(x.assignment, y.assignment))
        .slice(0, MAX_ITEMS);
      setItems(upcoming);
    } catch {
      setItems([]);
    }
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
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [refresh]);

  // Don't render the card at all until we know there's something to show —
  // an empty "Due soon" panel would be visual noise the design doesn't have.
  if (!items || items.length === 0) return null;

  const now = Date.now();

  return (
    <div className="pencil-DueSoon">
      <div className="pencil-dueHdr">
        <span className="icon is-small">
          <i className="fas fa-clock"></i>
        </span>
        <span className="pencil-dueHdr-label">Due soon</span>
      </div>
      {items.map(({ assignment: a, course: c }) => {
        const due = new Date(a.due_date!);
        const soon = due.getTime() - now <= SOON_MS;
        return (
          <Link
            key={a.id}
            to={`/course/${c.org_unit_id}/assignments/${a.id}`}
            className="pencil-due-row"
          >
            <span className="icon is-small" style={{ color: "var(--pencil-text-secondary)" }}>
              <i className="far fa-circle"></i>
            </span>
            <div className="pencil-due-row-col">
              <span className="pencil-due-row-title">{a.name}</span>
              <span className={`pencil-due-row-meta${soon ? " is-warn" : ""}`}>
                {fmtAssignmentDueDate(a.due_date)}
              </span>
            </div>
          </Link>
        );
      })}
    </div>
  );
}
