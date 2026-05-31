import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import type { Assignment } from "../types";
import SyntheticTaskModal from "./SyntheticTaskModal";
import { fmtAssignmentDueDate } from "../lib/format";

// Course Overview's quick-look at the user's hand-rolled tasks for this
// course. Auto-extraction (split-on-headings, etc.) is deferred — this
// is the "I can add things again" cut.

interface Props {
  courseId: string;
}


export default function SyntheticTasksPanel({ courseId }: Props) {
  const [items, setItems] = useState<Assignment[] | null>(null);
  const [showCreate, setShowCreate] = useState(false);

  const load = useCallback(() => {
    api
      .listAssignments(courseId, undefined)
      .then((all) => setItems(all.filter((a) => a.synthetic)))
      .catch(() => setItems([]));
  }, [courseId]);

  useEffect(() => { load(); }, [load]);

  async function toggleComplete(a: Assignment) {
    await api.toggleAssignmentComplete(a.id);
    load();
  }

  async function onDelete(a: Assignment) {
    await api.deleteAssignment(a.id);
    load();
  }

  return (
    <div className="box">
      <div className="is-flex is-justify-content-space-between is-align-items-center mb-3">
        <h2 className="title is-6 mb-0">
          <i className="fas fa-list-check mr-2 has-text-grey"></i>My tasks
        </h2>
        <button className="button is-small is-primary" onClick={() => setShowCreate(true)}>
          <span className="icon"><i className="fas fa-plus"></i></span>
          <span>Add task</span>
        </button>
      </div>

      {items === null ? (
        <p className="has-text-grey is-size-7 py-2">Loading…</p>
      ) : items.length === 0 ? (
        <p className="has-text-grey is-size-7 py-2">No personal tasks yet. Add one to track something Brightspace doesn't cover.</p>
      ) : (
        <table className="table is-fullwidth is-narrow" style={{ background: "transparent" }}>
          <tbody>
            {items.map((a) => (
              <tr key={a.id} className={a.completed ? "has-text-grey-light" : undefined}>
                <td style={{ width: 32 }}>
                  <input
                    type="checkbox"
                    checked={a.completed}
                    onChange={() => toggleComplete(a)}
                    title={a.completed ? "Mark incomplete" : "Mark complete"}
                  />
                </td>
                <td>
                  <Link
                    to={`/course/${courseId}/assignments/${a.id}`}
                    className={a.completed ? "has-text-grey" : "has-text-link-dark"}
                    style={a.completed ? { textDecoration: "line-through" } : undefined}
                  >
                    {a.name}
                  </Link>
                  {a.optional && <span className="tag is-light is-small ml-2">optional</span>}
                </td>
                <td className="has-text-grey is-size-7" style={{ whiteSpace: "nowrap" }}>{fmtAssignmentDueDate(a.due_date)}</td>
                <td style={{ width: 32 }}>
                  <button
                    className="button is-small is-white"
                    title="Delete this task"
                    aria-label="Delete this task"
                    onClick={() => onDelete(a)}
                  >
                    <span className="icon is-small has-text-grey"><i className="fas fa-trash"></i></span>
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <SyntheticTaskModal
        courseId={courseId}
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreated={() => load()}
      />
    </div>
  );
}
