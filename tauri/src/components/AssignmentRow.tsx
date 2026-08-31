import { Link } from "react-router-dom";
import { courseLabel, type Assignment, type Course } from "../types";
import BrightspaceLink, { useBrightspaceHost } from "./BrightspaceLink";
import { assignmentSubmitUrl } from "../lib/brightspace";
import { fmtAssignmentDueDate } from "../lib/format";

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
  // Optional. Pass for synthetic rows so the user can remove them inline.
  onDelete?: () => void;
  // Optional. Opens the edit modal for this row — any assignment, not just
  // synthetic ones.
  onEdit?: () => void;
}

export default function AssignmentRow({ assignment: a, course: c, leaving, onToggleComplete, showCourse, onDelete, onEdit }: Props) {
  const cls = `assignment-row is-flex is-justify-content-space-between is-align-items-center py-2${leaving ? " is-leaving" : ""}`;
  const detailHref = `/course/${a.course_id}/assignments/${a.id}`;
  const bsHost = useBrightspaceHost();
  const bsUrl = a.external_url ?? (bsHost ? assignmentSubmitUrl(bsHost, a.course_id, a.brightspace_id) : null);
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
              <strong>{courseLabel(c)}</strong>
            </Link>
            <span className="mx-2 has-text-grey-light">·</span>
          </>
        )}
        <Link to={detailHref} className="has-text-link-dark">{a.name}</Link>
        {bsUrl && <BrightspaceLink url={bsUrl} label="Open this assignment in Brightspace" className="is-inline-block" />}
        {a.completed && <span className="tag is-success is-light ml-2">done</span>}
        {a.optional && <span className="tag is-light ml-2">optional</span>}
        {a.synthetic && <span className="tag is-info is-light ml-2">synthetic</span>}
      </div>
      <span className="has-text-grey is-size-7" style={{ flex: "0 0 auto" }}>
        {fmtAssignmentDueDate(a.due_date)}
      </span>
      {onEdit && (
        <button
          className="button is-small is-white"
          title="Edit this assignment"
          aria-label="Edit assignment"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onEdit();
          }}
          style={{ flex: "0 0 auto" }}
        >
          <span className="icon is-small has-text-grey"><i className="fas fa-pen"></i></span>
        </button>
      )}
      {onDelete && (
        <button
          className="button is-small is-white"
          title="Delete this synthetic task"
          aria-label="Delete synthetic task"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            // window.confirm() is silently swallowed by WKWebView; trust the
            // click instead. If you misclick, recreate it — it's two fields.
            onDelete();
          }}
          style={{ flex: "0 0 auto" }}
        >
          <span className="icon is-small has-text-grey"><i className="fas fa-trash"></i></span>
        </button>
      )}
    </div>
  );
}
