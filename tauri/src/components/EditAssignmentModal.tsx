import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Assignment } from "../types";

// Modal for editing any assignment's user-facing fields — name, due date,
// notes/instructions and external URL. Works for Brightspace-sourced rows as
// well as synthetic ones: the backend marks edited rows manually_edited, so
// the Brightspace sync preserves the edits instead of overwriting them.

interface Props {
  /// The assignment being edited, or null when the modal is closed.
  assignment: Assignment | null;
  onClose: () => void;
  onSaved: (a: Assignment) => void;
}

// due_date can be an ISO-8601 UTC string (Brightspace rows) or the local-naive
// "YYYY-MM-DDTHH:mm:ss" a datetime-local input produced (synthetic rows).
// Date() parses both; format back to what <input type="datetime-local"> wants.
function toLocalInput(raw: string | null): string {
  if (!raw) return "";
  const d = new Date(raw);
  if (Number.isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export default function EditAssignmentModal({ assignment: a, onClose, onSaved }: Props) {
  const [name, setName] = useState("");
  const [due, setDue] = useState("");
  // Only rewrite due_date when the user touched the input — round-tripping an
  // untouched value through the datetime-local format would drop seconds/zone.
  const [dueDirty, setDueDirty] = useState(false);
  const [desc, setDesc] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Seed the form when the modal opens (or switches rows) — not on every
  // parent re-render, which can hand us a fresh object for the same row and
  // would wipe in-progress typing.
  const seededIdRef = useRef<number | null>(null);
  useEffect(() => {
    const currId = a?.id ?? null;
    if (a && currId !== seededIdRef.current) {
      setName(a.name);
      setDue(toLocalInput(a.due_date));
      setDueDirty(false);
      setDesc(a.description ?? "");
      setUrl(a.external_url ?? "");
      setErr(null);
    }
    seededIdRef.current = currId;
  }, [a]);

  if (!a) return null;

  async function submit() {
    if (!a) return;
    if (!name.trim()) {
      setErr("Name required");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      // <input type="datetime-local"> emits "YYYY-MM-DDTHH:mm"; append :00
      // so the backend treats it as a fixed instant.
      const dueOut = dueDirty ? (due.trim() === "" ? null : `${due}:00`) : a.due_date;
      const updated = await api.updateAssignment(
        a.id,
        name.trim(),
        dueOut,
        desc.trim() || null,
        url.trim() || null,
      );
      onClose();
      onSaved(updated);
    } catch (e) {
      setErr(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal is-active">
      <div className="modal-background" onClick={onClose} />
      <div className="modal-card">
        <header className="modal-card-head">
          <p className="modal-card-title">Edit assignment</p>
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
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submit();
              }}
            />
          </div>
          <div className="field">
            <label className="label">Due</label>
            <input
              className="input"
              type="datetime-local"
              value={due}
              onChange={(e) => {
                setDue(e.target.value);
                setDueDirty(true);
              }}
            />
          </div>
          <div className="field">
            <label className="label">External URL</label>
            <input
              className="input"
              type="text"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://..."
            />
          </div>
          <div className="field">
            <label className="label">Notes / instructions</label>
            <textarea
              className="textarea"
              rows={5}
              value={desc}
              onChange={(e) => setDesc(e.target.value)}
              placeholder="Markdown is fine here — **bold**, lists, links, etc."
            />
            <p className="help">Renders as markdown (or HTML) on the assignment page.</p>
          </div>
          {!a.synthetic && (
            <p className="help">
              Your edits are kept when Brilliant re-syncs this assignment from Brightspace.
            </p>
          )}
          {err && <p className="help is-danger">{err}</p>}
        </section>
        <footer className="modal-card-foot">
          <button className="button is-primary" disabled={busy} onClick={submit}>
            {busy ? "Saving…" : "Save changes"}
          </button>
          <button className="button" disabled={busy} onClick={onClose}>
            Cancel
          </button>
        </footer>
      </div>
    </div>
  );
}
