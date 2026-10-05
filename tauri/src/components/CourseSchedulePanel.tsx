import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { CourseMeetingInfo } from "../lib/syllabusRoom";

interface Props {
  courseId: string;
  info: CourseMeetingInfo | null;
  customDays?: string | null;
  customTime?: string | null;
  onScheduleUpdated?: (days: string | null, time: string | null) => void;
}

export default function CourseSchedulePanel({ courseId, info, customDays = null, customTime = null, onScheduleUpdated }: Props) {
  const [override, setOverride] = useState({ courseId, days: customDays, time: customTime });
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState({ days: "", time: "" });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const activeCourse = useRef(courseId);
  activeCourse.current = courseId;
  useEffect(() => { setOverride({ courseId, days: customDays, time: customTime }); }, [courseId, customDays, customTime]);
  useEffect(() => {
    activeCourse.current = courseId;
    setEditing(false); setSaving(false); setError(null);
    return () => { activeCourse.current = ""; };
  }, [courseId]);
  const manual = override.courseId === courseId ? override : { days: customDays, time: customTime };
  const days = manual.days ?? info?.days;
  const time = manual.time ?? info?.time;

  async function save(nextDays: string | null, nextTime: string | null) {
    if (saving) return;
    const id = courseId;
    const d = nextDays?.trim() || null;
    const t = nextTime?.trim() || null;
    setSaving(true); setError(null);
    try {
      await api.updateCourseSchedule(id, d, t);
      if (activeCourse.current !== id) return;
      setOverride({ courseId: id, days: d, time: t });
      onScheduleUpdated?.(d, t);
      setEditing(false);
    } catch {
      if (activeCourse.current === id) setError("Could not save the class schedule. Try again.");
    } finally {
      if (activeCourse.current === id) setSaving(false);
    }
  }

  function display(value: string | undefined | null, edited: boolean, source: string | undefined, missing: string) {
    return value ? <><p className="has-text-weight-semibold">{value}</p><p className="help">{edited ? "Edited by you" : `From ${source}`}</p></>
      : <p className="has-text-grey is-size-7">{!info ? "Checking syllabus…" : info.error ?? missing}</p>;
  }

  return <section className="box" aria-live="polite" aria-label="Class schedule">
    <h2 className="title is-6 mb-2"><i className="fas fa-clock mr-2 has-text-grey" aria-hidden="true"></i>Class schedule</h2>
    {editing ? <form onSubmit={(event) => { event.preventDefault(); void save(draft.days, draft.time); }}
      onKeyDown={(event) => { if (event.key === "Escape" && !saving) { setEditing(false); setError(null); } }}>
      <div className="field">
        <label className="label is-small" htmlFor={`days-${courseId}`}>Days of week</label>
        <input id={`days-${courseId}`} className="input" value={draft.days} autoFocus disabled={saving}
          placeholder={info?.days ?? "e.g. Monday, Wednesday"} onChange={(event) => setDraft({ ...draft, days: event.target.value })} />
      </div>
      <div className="field">
        <label className="label is-small" htmlFor={`time-${courseId}`}>Meeting time</label>
        <input id={`time-${courseId}`} className="input" value={draft.time} disabled={saving}
          placeholder={info?.time ?? "e.g. 9:30–10:45 AM"} onChange={(event) => setDraft({ ...draft, time: event.target.value })} />
      </div>
      <p className="help">Your changes are kept when the syllabus updates. Leave a field blank to use its syllabus value.</p>
      <div className="buttons mt-3 mb-0">
        <button className="button is-small is-primary" type="submit" disabled={saving}>{saving ? "Saving…" : "Save schedule"}</button>
        <button className="button is-small" type="button" disabled={saving} onClick={() => { setEditing(false); setError(null); }}>Cancel</button>
      </div>
    </form> : <>
      <div className="columns mb-0">
        <div className="column"><h3 className="label is-small">Days of week</h3>{display(days, manual.days !== null, info?.daysSource, "No class days found in the syllabus.")}</div>
        <div className="column"><h3 className="label is-small">Meeting time</h3>{display(time, manual.time !== null, info?.timeSource, "No meeting time found in the syllabus.")}</div>
      </div>
      <div className="buttons mt-3 mb-0">
        <button className="button is-small" type="button" disabled={saving} onClick={() => {
          // Keep automatic fields blank so editing one field leaves the other automatic.
          setDraft({ days: manual.days ?? "", time: manual.time ?? "" }); setError(null); setEditing(true);
        }}>{days || time ? "Edit schedule" : "Set schedule"}</button>
        {(manual.days !== null || manual.time !== null) && <button className="button is-small is-light" type="button" disabled={saving} onClick={() => save(null, null)}>Use syllabus schedule</button>}
      </div>
    </>}
    {error && <p className="help is-danger" role="alert">{error}</p>}
  </section>;
}
