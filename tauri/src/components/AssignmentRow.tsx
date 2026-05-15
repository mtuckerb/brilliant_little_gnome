import { Link } from "react-router-dom";
import { displayCourseName, type Assignment, type Course } from "../types";

// Single-row presentation used by both Calendar and Assignments. Caller owns
// "show completed" state and the leaving-animation set; this component is just
// markup + a click handler.
interface Props {
  assignment: Assignment;
  course?: Course;
  leaving?: boolean;
  onToggleComplete: (a: Assignment) => void;
  // When set, show course name/code in the row (Calendar). When omitted,
  // assume the row is already inside a course context (Assignments page).
  showCourse?: boolean;
}

export default function AssignmentRow({ assignment: a, course: c, leaving, onToggleComplete, showCourse }: Props) {
  const cls = `assignment-row is-flex is-justify-content-space-between is-align-items-center py-2${leaving ? " is-leaving" : ""}`;
  const detailHref = `/course/${a.course_id}/assignments/${a.id}`;
  return (
    <div className={cls} style={{ borderBottom: "1px solid #eee", gap: 12 }}>
      <input
        type="checkbox"
        checked={a.completed}
        onChange={() => onToggleComplete(a)}
        title={a.completed ? "Mark incomplete" : "Mark complete"}
        style={{ flex: "0 0 auto" }}
      />
      <div style={{ flex: "1 1 auto", minWidth: 0 }}>
        {showCourse && c && (
          <>
            <Link to={`/course/${c.org_unit_id}/assignments`} style={{ color: c.custom_color || undefined }}>
              <strong>{c.code ? `${c.code} - ${displayCourseName(c)}` : displayCourseName(c)}</strong>
            </Link>
            <span className="mx-2 has-text-grey-light">·</span>
          </>
        )}
        <Link to={detailHref} className="has-text-link-dark">{a.name}</Link>
        {a.completed && <span className="tag is-success is-light ml-2">done</span>}
        {a.optional && <span className="tag is-light ml-2">optional</span>}
        {a.synthetic && <span className="tag is-info is-light ml-2">synthetic</span>}
      </div>
      <span className="has-text-grey is-size-7" style={{ flex: "0 0 auto" }}>
        {a.due_date && new Date(a.due_date).toLocaleString(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" })}
      </span>
    </div>
  );
}
