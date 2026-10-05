import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import CourseSchedulePanel from "./CourseSchedulePanel";
import { findCourseMeetingInfo, parseCourseMeetingInfo, type CourseMeetingInfo } from "../lib/syllabusRoom";

interface Props {
  courseId: string;
  revision?: number;
  cachedMeetingInfo?: string | null;
  onMeetingInfoUpdated?: (info: string) => void;
  customDays?: string | null;
  customTime?: string | null;
  onScheduleUpdated?: (days: string | null, time: string | null) => void;
  customRoom?: string | null;
  onRoomUpdated?: (room: string | null) => void;
}

export default function CourseRoomPanel({ courseId, revision = 0, customRoom = null, onRoomUpdated, customDays, customTime, onScheduleUpdated, cachedMeetingInfo, onMeetingInfoUpdated }: Props) {
  const [result, setResult] = useState<{ courseId: string; info: CourseMeetingInfo; error?: string } | null>(null);
  const [override, setOverride] = useState({ courseId, room: customRoom });
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const cachedInfoRef = useRef(cachedMeetingInfo);
  cachedInfoRef.current = cachedMeetingInfo;
  const activeCourse = useRef(courseId);
  activeCourse.current = courseId;
  useEffect(() => {
    setOverride({ courseId, room: customRoom });
  }, [courseId, customRoom]);
  useEffect(() => {
    activeCourse.current = courseId;
    setEditing(false);
    setSaving(false);
    setSaveError(null);
    return () => { activeCourse.current = ""; };
  }, [courseId]);
  useEffect(() => {
    let cancelled = false;
    const cacheAtStart = cachedInfoRef.current;
    setResult(null);
    findCourseMeetingInfo(courseId)
      .then(async (info) => {
        if (cancelled) return;
        if (cachedInfoRef.current !== cacheAtStart) {
          setResult({ courseId, info: parseCourseMeetingInfo(cachedInfoRef.current) ?? info });
          return;
        }
        if (info.readable || info.room || info.days || info.time) {
          try {
            const shared = await api.cacheCourseMeetingInfo(courseId, info, !!info.readable && !info.error);
            if (cancelled) return;
            onMeetingInfoUpdated?.(JSON.stringify(shared));
            info = { ...shared, error: info.error };
          } catch {
            if (cancelled) return;
            setSaveError("Could not save syllabus details for device sync. Sync this course to try again.");
          }
        }
        if (!cancelled) setResult({ courseId, info });
      })
      .catch(() => { if (!cancelled) setResult({ courseId, info: {}, error: "Could not read the syllabus. Sync this course to try again." }); });
    return () => { cancelled = true; };
  }, [courseId, revision]);
  const shared = useMemo(() => parseCourseMeetingInfo(cachedMeetingInfo), [cachedMeetingInfo]);
  const current = result?.courseId === courseId
    ? { ...result, info: shared ? { ...shared, error: result.info.error } : result.info }
    : shared ? { courseId, info: shared, error: undefined } : null;
  const manualRoom = override.courseId === courseId ? override.room : customRoom;
  const displayedRoom = manualRoom ?? current?.info.room;

  function startEditing() {
    setDraft(displayedRoom ?? "");
    setSaveError(null);
    setEditing(true);
  }

  async function saveRoom(room: string | null) {
    if (saving) return;
    const id = courseId;
    const next = room?.trim() || null;
    setSaving(true);
    setSaveError(null);
    try {
      await api.updateCourseRoom(id, next);
      if (activeCourse.current !== id) return;
      setOverride({ courseId: id, room: next });
      onRoomUpdated?.(next);
      setEditing(false);
    } catch {
      if (activeCourse.current === id) setSaveError("Could not save the room number. Try again.");
    } finally {
      if (activeCourse.current === id) setSaving(false);
    }
  }

  return (
    <>
    <div className="box" aria-live="polite">
      <h2 className="title is-6 mb-2"><i className="fas fa-door-open mr-2 has-text-grey" aria-hidden="true"></i>Room number</h2>
      {editing ? (
        <form onSubmit={(event) => { event.preventDefault(); void saveRoom(draft); }}>
          <label className="label is-small" htmlFor={`room-${courseId}`}>Room number or location</label>
          <input id={`room-${courseId}`} className="input" type="text" value={draft}
            onChange={(event) => setDraft(event.target.value)} autoFocus disabled={saving}
            placeholder="e.g. Science Hall 214"
            onKeyDown={(event) => { if (event.key === "Escape" && !saving) { setEditing(false); setSaveError(null); } }} />
          <p className="help">Your change is kept when the syllabus updates. Leave blank to use the syllabus.</p>
          <div className="buttons mt-3 mb-0">
            <button type="submit" className="button is-small is-primary" disabled={saving}>{saving ? "Saving…" : "Save"}</button>
            <button type="button" className="button is-small" disabled={saving} onClick={() => { setEditing(false); setSaveError(null); }}>Cancel</button>
          </div>
        </form>
      ) : <>
      {displayedRoom ? <><p className="has-text-weight-semibold">{displayedRoom}</p><p className="help">{manualRoom !== null ? "Edited by you" : `From ${current?.info.roomSource ?? "syllabus"}`}</p></>
        : !current ? <p className="has-text-grey is-size-7">Checking syllabus…</p>
        : <p className="has-text-grey is-size-7">{current.error ?? current.info.error ?? "No room number found in the syllabus."}</p>}
      <div className="buttons mt-3 mb-0">
        <button type="button" className="button is-small" disabled={saving} onClick={startEditing}>{displayedRoom ? "Edit room" : "Set room"}</button>
        {manualRoom !== null && <button type="button" className="button is-small is-light" disabled={saving} onClick={() => saveRoom(null)}>{saving ? "Saving…" : "Use syllabus"}</button>}
      </div>
      </>}
      {saveError && <p className="help is-danger" role="alert">{saveError}</p>}
    </div>
    <CourseSchedulePanel courseId={courseId} info={current ? { ...current.info, error: current.error ?? current.info.error } : null}
      customDays={customDays} customTime={customTime} onScheduleUpdated={onScheduleUpdated} />
    </>
  );
}
