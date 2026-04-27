import { useEffect, useState, useCallback } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { Assignment } from "../types";

export default function Assignments() {
  const { id } = useParams<{ id: string }>();
  const [items, setItems] = useState<Assignment[] | null>(null);
  const [showCompleted, setShowCompleted] = useState(false);

  const load = useCallback(() => {
    if (!id) return;
    api.listAssignments(id, showCompleted ? undefined : false).then(setItems);
  }, [id, showCompleted]);

  useEffect(() => { load(); }, [load]);

  if (!items) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  return (
    <div>
      <nav className="breadcrumb mb-3">
        <ul>
          <li><Link to="/dashboard">Dashboard</Link></li>
          <li><Link to={`/course/${id}`}>Course</Link></li>
          <li className="is-active"><a>Assignments</a></li>
        </ul>
      </nav>
      <div className="box">
        <div className="level">
          <div className="level-left"><h2 className="title is-4"><i className="fas fa-tasks mr-2"></i>Assignments</h2></div>
          <div className="level-right">
            <button className="button is-small" onClick={() => setShowCompleted((s) => !s)}>
              {showCompleted ? "Hide completed" : "Show completed"}
            </button>
          </div>
        </div>
        <table className="table is-fullwidth is-hoverable">
          <thead>
            <tr><th></th><th>Name</th><th>Due</th><th>Type</th></tr>
          </thead>
          <tbody>
            {items.map((a) => (
              <tr key={a.id}>
                <td>
                  <input type="checkbox" checked={a.completed} onChange={() => api.toggleAssignmentComplete(a.id).then(load)} />
                </td>
                <td>{a.name}{a.synthetic && <span className="tag is-small is-info ml-2">Synthetic</span>}{a.optional && <span className="tag is-small is-light ml-2">Optional</span>}</td>
                <td>{a.due_date ? new Date(a.due_date).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }) : "-"}</td>
                <td>{a.assignment_type ?? "-"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
