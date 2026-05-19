import { useEffect, useState, useCallback } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { Assignment } from "../types";
import AssignmentRow from "../components/AssignmentRow";
import CourseHeader from "../components/CourseHeader";
import SyntheticTaskModal from "../components/SyntheticTaskModal";

const EXIT_MS = 280;

export default function Assignments() {
  const { id } = useParams<{ id: string }>();
  const [items, setItems] = useState<Assignment[] | null>(null);
  const [showCompleted, setShowCompleted] = useState(false);
  const [leaving, setLeaving] = useState<Set<number>>(new Set());
  const [showCreate, setShowCreate] = useState(false);

  const load = useCallback(() => {
    if (!id) return;
    api.listAssignments(id, undefined).then(setItems);
  }, [id]);

  useEffect(() => { load(); }, [load]);

  if (!items) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

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

  async function onDelete(a: Assignment) {
    if (!a.synthetic) return;
    await api.deleteAssignment(a.id);
    load();
  }

  const visible = items.filter((a) => !a.completed || showCompleted || leaving.has(a.id));

  return (
    <div>
      <nav className="breadcrumb mb-3">
        <ul>
          <li><Link to="/dashboard">Dashboard</Link></li>
          <li><Link to={`/course/${id}`}>Course</Link></li>
          <li className="is-active"><a>Assignments</a></li>
        </ul>
      </nav>
      {id && <CourseHeader courseId={id} />}
      <div className="box">
        <div className="level">
          <div className="level-left"><h2 className="title is-4"><i className="fas fa-tasks mr-2"></i>Assignments</h2></div>
          <div className="level-right">
            <button className="button is-small is-primary mr-2" onClick={() => setShowCreate(true)}>
              <span className="icon"><i className="fas fa-plus"></i></span>
              <span>Add task</span>
            </button>
            <button className="button is-small" onClick={() => setShowCompleted((s) => !s)}>
              <span className="icon"><i className={`fas ${showCompleted ? "fa-eye-slash" : "fa-eye"}`}></i></span>
              <span>{showCompleted ? "Hide completed" : "Show completed"}</span>
            </button>
          </div>
        </div>
        {visible.length === 0 ? (
          <p className="has-text-grey py-4">No assignments.</p>
        ) : (
          visible.map((a) => (
            <AssignmentRow
              key={a.id}
              assignment={a}
              leaving={leaving.has(a.id)}
              onToggleComplete={onToggleComplete}
              onDelete={a.synthetic ? () => onDelete(a) : undefined}
            />
          ))
        )}
      </div>

      {id && (
        <SyntheticTaskModal
          courseId={id}
          open={showCreate}
          onClose={() => setShowCreate(false)}
          onCreated={() => load()}
        />
      )}
    </div>
  );
}
