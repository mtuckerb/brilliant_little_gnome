import { useEffect, useState } from "react";
import { api } from "../api";
import type { Assignment } from "../types";

// Modal for creating a user-defined ("synthetic") assignment. Extracted from
// the Assignments page so the Overview page can use the same form without
// duplicating the markup. Notes accept markdown — RichText renders that on
// the detail page.

interface Props {
  courseId: string;
  open: boolean;
  onClose: () => void;
  onCreated: (a: Assignment) => void;
  /// Optional pre-filled values — used by "Create task from announcement"
  /// so the user lands on a partially-completed form.
  initialName?: string;
  initialDescription?: string;
}

export default function SyntheticTaskModal({ courseId, open, onClose, onCreated, initialName, initialDescription }: Props) {
  const [name, setName] = useState(initialName ?? "");
  const [due, setDue] = useState("");
  const [desc, setDesc] = useState(initialDescription ?? "");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Refresh the form when the modal re-opens with different initials
  // (otherwise the second announcement→task click reuses the first one).
  useEffect(() => {
    if (open) {
      setName(initialName ?? "");
      setDesc(initialDescription ?? "");
      setDue("");
      setErr(null);
    }
  }, [open, initialName, initialDescription]);

  function reset() {
    setName(initialName ?? "");
    setDue("");
    setDesc(initialDescription ?? "");
    setErr(null);
  }

  async function submit() {
    if (!name.trim()) {
      setErr("Name required");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      // <input type="datetime-local"> emits "YYYY-MM-DDTHH:mm"; append :00
      // so the backend treats it as a fixed instant.
      const dueIso = due.trim() === "" ? null : `${due}:00`;
      const created = await api.createSyntheticAssignment(
        courseId,
        name.trim(),
        dueIso,
        desc.trim() || null,
      );
      reset();
      onClose();
      onCreated(created);
    } catch (e) {
      setErr(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  if (!open) return null;

  return (
    <div className="modal is-active">
      <div className="modal-background" onClick={onClose} />
      <div className="modal-card">
        <header className="modal-card-head">
          <p className="modal-card-title">New synthetic task</p>
          <button className="delete" aria-label="close" onClick={onClose} />
        </header>
        <section className="modal-card-body">
          <div className="field">
            <label className="label">Name</label>
            <input
              className="input"
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Read chapter 3"
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submit();
              }}
            />
          </div>
          <div className="field">
            <label className="label">Due (optional)</label>
            <input
              className="input"
              type="datetime-local"
              value={due}
              onChange={(e) => setDue(e.target.value)}
            />
          </div>
          <div className="field">
            <label className="label">Notes (optional)</label>
            <textarea
              className="textarea"
              rows={4}
              value={desc}
              onChange={(e) => setDesc(e.target.value)}
              placeholder="Markdown is fine here — **bold**, lists, links, etc."
            />
            <p className="help">Renders as markdown on the task detail page.</p>
          </div>
          {err && <p className="help is-danger">{err}</p>}
        </section>
        <footer className="modal-card-foot">
          <button className="button is-primary" disabled={busy} onClick={submit}>
            {busy ? "Creating…" : "Create"}
          </button>
          <button className="button" disabled={busy} onClick={onClose}>
            Cancel
          </button>
        </footer>
      </div>
    </div>
  );
}
