import { useEffect, useState, useCallback } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { Assignment } from "../types";
import AssignmentRow from "../components/AssignmentRow";

const EXIT_MS = 280;

export default function Assignments() {
  const { id } = useParams<{ id: string }>();
  const [items, setItems] = useState<Assignment[] | null>(null);
  const [showCompleted, setShowCompleted] = useState(false);
  const [leaving, setLeaving] = useState<Set<number>>(new Set());
  const [showCreate, setShowCreate] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [draftDue, setDraftDue] = useState("");
  const [draftDesc, setDraftDesc] = useState("");
  const [creating, setCreating] = useState(false);
  const [createErr, setCreateErr] = useState<string | null>(null);

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

  function resetDraft() {
    setDraftName("");
    setDraftDue("");
    setDraftDesc("");
    setCreateErr(null);
  }

  async function submitCreate() {
    if (!id) return;
    if (!draftName.trim()) {
      setCreateErr("Name required");
      return;
    }
    setCreating(true);
    setCreateErr(null);
    try {
      // <input type="datetime-local"> emits "YYYY-MM-DDTHH:mm" with no timezone;
      // append seconds + Z so the backend (and JS Date downstream) treats it as
      // a fixed instant rather than guessing the local zone again.
      const due = draftDue.trim() === "" ? null : `${draftDue}:00`;
      await api.createSyntheticAssignment(id, draftName.trim(), due, draftDesc.trim() || null);
      setShowCreate(false);
      resetDraft();
      load();
    } catch (e) {
      setCreateErr(String((e as { message?: string })?.message ?? e));
    } finally {
      setCreating(false);
    }
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
      <div className="box">
        <div className="level">
          <div className="level-left"><h2 className="title is-4"><i className="fas fa-tasks mr-2"></i>Assignments</h2></div>
          <div className="level-right">
            <button className="button is-small is-primary mr-2" onClick={() => { resetDraft(); setShowCreate(true); }}>
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
            />
          ))
        )}
      </div>

      {showCreate && (
        <div className="modal is-active">
          <div className="modal-background" onClick={() => setShowCreate(false)} />
          <div className="modal-card">
            <header className="modal-card-head">
              <p className="modal-card-title">New synthetic task</p>
              <button className="delete" aria-label="close" onClick={() => setShowCreate(false)} />
            </header>
            <section className="modal-card-body">
              <div className="field">
                <label className="label">Name</label>
                <input className="input" autoFocus value={draftName} onChange={(e) => setDraftName(e.target.value)} placeholder="Read chapter 3" />
              </div>
              <div className="field">
                <label className="label">Due (optional)</label>
                <input className="input" type="datetime-local" value={draftDue} onChange={(e) => setDraftDue(e.target.value)} />
              </div>
              <div className="field">
                <label className="label">Notes (optional)</label>
                <textarea className="textarea" rows={3} value={draftDesc} onChange={(e) => setDraftDesc(e.target.value)} />
              </div>
              {createErr && <p className="help is-danger">{createErr}</p>}
            </section>
            <footer className="modal-card-foot">
              <button className="button is-primary" disabled={creating} onClick={submitCreate}>
                {creating ? "Creating…" : "Create"}
              </button>
              <button className="button" disabled={creating} onClick={() => setShowCreate(false)}>Cancel</button>
            </footer>
          </div>
        </div>
      )}
    </div>
  );
}
