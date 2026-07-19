import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { api } from "../api";
import { type Course } from "../types";
import SyllabusPanel from "../components/SyllabusPanel";
import SyntheticTasksPanel from "../components/SyntheticTasksPanel";
import { triggerDownload } from "../lib/download";
import HeaderBand from "../components/HeaderBand";
import { runZotero } from "../lib/zotero";
import { useToast } from "../components/ToastProvider";

const DAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

export default function CourseDetail() {
  const { id } = useParams<{ id: string }>();
  const [course, setCourse] = useState<Course | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [sendingZotero, setSendingZotero] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [caching, setCaching] = useState(false);
  const [cacheEnabled, setCacheEnabled] = useState(true);
  const [cacheStatus, setCacheStatus] = useState<{ count: number; bytes: number } | null>(null);
  const toast = useToast();
  const navigate = useNavigate();

  useEffect(() => {
    if (!id) return;
    api.getCourse(id).then(setCourse).catch((e) => setErr(String(e?.message ?? e)));
    api.getPrefs().then((p) => setCacheEnabled(p.cache_content)).catch(() => {});
    api.courseCacheStatus(id).then(setCacheStatus).catch(() => {});
  }, [id]);

  const fmtBytes = (b: number) =>
    b >= 1 << 20 ? `${(b / (1 << 20)).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`;

  async function onCacheOffline() {
    if (!course) return;
    setCaching(true);
    try {
      const r = await api.cacheCourseContent(course.org_unit_id);
      toast.show(
        `Cached ${r.cached} item${r.cached === 1 ? "" : "s"} (${fmtBytes(r.bytes)})` +
          (r.failed ? `, ${r.failed} failed` : "") +
          `. ${r.skipped} skipped (quizzes/links).`,
        r.failed ? "is-warning" : "is-success",
        7000,
      );
      setCacheStatus(await api.courseCacheStatus(course.org_unit_id));
    } catch (e) {
      toast.show(String((e as { message?: string })?.message ?? e), "is-danger", 8000);
    } finally {
      setCaching(false);
    }
  }

  async function onClearCache() {
    if (!course) return;
    await api.clearCourseCache(course.org_unit_id).catch(() => {});
    setCacheStatus(await api.courseCacheStatus(course.org_unit_id).catch(() => ({ count: 0, bytes: 0 })));
    toast.show("Cleared this course's offline cache.", "is-info", 4000);
  }

  if (err) return <div className="notification is-danger">{err}</div>;
  if (!course) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  // Local color edits commit on `change` (color pickers don't fire on every drag
  // step); we optimistically reflect the change so the picker doesn't lag.
  async function onColorChange(next: string) {
    if (!course) return;
    setCourse({ ...course, custom_color: next });
    await api.updateCourseColor(course.org_unit_id, next).catch(() => {});
  }

  async function onTargetChange(raw: string) {
    if (!course) return;
    const n = raw.trim() === "" ? null : parseFloat(raw);
    if (n !== null && (Number.isNaN(n) || n < 0 || n > 100)) return;
    setCourse({ ...course, target_grade: n });
    await api.updateCourseTargetGrade(course.org_unit_id, n).catch(() => {});
  }

  async function onUnitsChange(raw: string) {
    if (!course) return;
    const n = raw.trim() === "" ? null : parseFloat(raw);
    if (n !== null && (Number.isNaN(n) || n < 0)) return;
    setCourse({ ...course, units: n });
    await api.updateCourseUnits(course.org_unit_id, n).catch(() => {});
  }

  async function onEndOfWeekChange(raw: string) {
    if (!course) return;
    const n = parseInt(raw, 10);
    if (Number.isNaN(n) || n < 0 || n > 6) return;
    setCourse({ ...course, end_of_week_day: n });
    await api.updateCourseEndOfWeek(course.org_unit_id, n).catch(() => {});
  }

  async function onDownloadEverything() {
    if (!course || downloading) return;
    // window.confirm() returns immediately false in Tauri 2 WKWebView so the
    // click was being silently swallowed. The button itself is the
    // confirmation — clicking it intentionally is enough; tray feedback +
    // "Bundling…" state makes the long-running work visible.
    console.info("download_course_archive starting", course.org_unit_id);
    setDownloading(true);
    try {
      const payload = await api.downloadCourseArchive(course.org_unit_id);
      console.info("download_course_archive done", payload);
      triggerDownload(payload);
      const where = payload.saved_path ? ` → ${payload.saved_path}` : " to your Downloads folder";
      toast.show(`Saved ${payload.filename}${where}`, "is-success", 7000);
    } catch (e) {
      console.error("download_course_archive failed", e);
      const msg = String((e as { message?: string })?.message ?? e);
      if (msg.toLowerCase().includes("authentication required")) {
        toast.show(
          "Your Brightspace session expired. Sign in again (status indicator, top right), then retry the download.",
          "is-danger",
          8000,
        );
      } else {
        toast.show(`Download failed: ${msg}`, "is-danger", 6000);
      }
    } finally {
      setDownloading(false);
    }
  }

  async function onSendCourseToZotero() {
    if (!course || sendingZotero) return;
    setSendingZotero(true);
    await runZotero(toast, "Whole course", () =>
      api.zoteroSendCourse(course.org_unit_id),
    );
    setSendingZotero(false);
  }

  async function onDeleteForever() {
    if (!course) return;
    try {
      await api.deleteCourse(course.org_unit_id);
      navigate("/dashboard");
    } catch (e) {
      toast.show(
        `Delete failed: ${String((e as { message?: string })?.message ?? e)}`,
        "is-danger",
      );
    }
  }

  async function onSync() {
    if (!course || syncing) return;
    setSyncing(true);
    try {
      await api.syncCourse(course.org_unit_id);
      const fresh = await api.getCourse(course.org_unit_id);
      setCourse(fresh);
    } finally {
      setSyncing(false);
    }
  }

  return (
    <div>
      <HeaderBand courseId={course.org_unit_id} onCourseUpdated={setCourse} />

      <div className="is-flex is-align-items-center is-flex-wrap-wrap mb-4" style={{ gap: 12 }}>
        <label className="is-flex is-align-items-center" style={{ gap: 8 }}>
          <span className="is-size-7 has-text-grey">Color</span>
          <input
            type="color"
            value={course.custom_color || "#739AC3"}
            onChange={(e) => onColorChange(e.target.value)}
            style={{ width: 36, height: 28, padding: 0, border: "1px solid #ccc", borderRadius: 4 }}
            title="Course color"
          />
        </label>
        <button className="button is-small is-light" onClick={onSync} disabled={syncing}>
          <span className="icon"><i className={`fas fa-sync ${syncing ? "fa-spin" : ""}`}></i></span>
          <span>{syncing ? "Syncing…" : "Sync"}</span>
        </button>
        <button
          className="button is-small is-light"
          onClick={onDownloadEverything}
          disabled={downloading}
          title="Download every content file in this course as a ZIP"
        >
          <span className="icon"><i className={`fas ${downloading ? "fa-circle-notch fa-spin" : "fa-file-archive"}`}></i></span>
          <span>{downloading ? "Bundling…" : "Download all"}</span>
        </button>
        <button
          className="button is-small is-light"
          onClick={onSendCourseToZotero}
          disabled={sendingZotero}
          title="Send every content file in this course to your Zotero library (creates a per-course collection)"
        >
          <span className="icon"><i className={`fas ${sendingZotero ? "fa-circle-notch fa-spin" : "fa-book-bookmark"}`}></i></span>
          <span>{sendingZotero ? "Sending…" : "Send to Zotero"}</span>
        </button>
        {cacheEnabled && (
          <button
            className="button is-small is-light"
            onClick={onCacheOffline}
            disabled={caching}
            title="Cache this course's files, Tools, and media on this device for offline use (quizzes are skipped)"
          >
            <span className="icon"><i className={`fas ${caching ? "fa-circle-notch fa-spin" : "fa-cloud-arrow-down"}`}></i></span>
            <span>
              {caching
                ? "Caching…"
                : cacheStatus && cacheStatus.count > 0
                  ? `Offline: ${cacheStatus.count} (${fmtBytes(cacheStatus.bytes)})`
                  : "Make available offline"}
            </span>
          </button>
        )}
        {cacheEnabled && cacheStatus && cacheStatus.count > 0 && !caching && (
          <button className="button is-small is-white" onClick={onClearCache} title="Remove this course's offline cache">
            <span className="icon has-text-grey"><i className="fas fa-trash-can"></i></span>
          </button>
        )}
      </div>

      <SyllabusPanel courseId={course.org_unit_id} />

      <SyntheticTasksPanel courseId={course.org_unit_id} />

      <div className="box">
        <h2 className="title is-6 mb-3"><i className="fas fa-sliders-h mr-2 has-text-grey"></i>Course settings</h2>
        <div className="columns is-multiline">
          <div className="column is-one-third">
            <label className="label is-small">Target grade (%)</label>
            <input
              className="input"
              type="number"
              min={0}
              max={100}
              step={0.5}
              defaultValue={course.target_grade ?? ""}
              onBlur={(e) => onTargetChange(e.target.value)}
              placeholder="93"
            />
            <p className="help">Used to compute the average needed on remaining work.</p>
          </div>
          <div className="column is-one-third">
            <label className="label is-small">Units / Credits</label>
            <input
              className="input"
              type="number"
              min={0}
              step={0.5}
              defaultValue={course.units ?? ""}
              onBlur={(e) => onUnitsChange(e.target.value)}
              placeholder="3"
            />
            <p className="help">Used for GPA weighting.</p>
          </div>
          <div className="column is-one-third">
            <label className="label is-small">End of week</label>
            <div className="select is-fullwidth">
              <select
                value={course.end_of_week_day ?? 0}
                onChange={(e) => onEndOfWeekChange(e.target.value)}
              >
                {DAY_NAMES.map((d, i) => (
                  <option key={i} value={i}>{d}</option>
                ))}
              </select>
            </div>
            <p className="help">Default due day for synthetic tasks.</p>
          </div>
        </div>
      </div>

      <div className="box" style={{ borderColor: "#f5c6cb" }}>
        <h2 className="title is-6 mb-3 has-text-danger">
          <i className="fas fa-triangle-exclamation mr-2"></i>Danger zone
        </h2>
        {confirmDelete ? (
          <div className="is-flex is-align-items-center is-flex-wrap-wrap" style={{ gap: 8 }}>
            <span className="is-size-7">
              Permanently delete <strong>{course.custom_name || course.name}</strong> and every assignment, grade, module, and discussion synced for it. The next Brightspace sync will recreate it if you're still enrolled.
            </span>
            <button className="button is-small is-danger" onClick={onDeleteForever}>
              <span className="icon"><i className="fas fa-trash"></i></span>
              <span>Yes, delete forever</span>
            </button>
            <button className="button is-small" onClick={() => setConfirmDelete(false)}>Cancel</button>
          </div>
        ) : (
          <button className="button is-small is-danger is-light" onClick={() => setConfirmDelete(true)}>
            <span className="icon"><i className="fas fa-trash"></i></span>
            <span>Delete this course forever…</span>
          </button>
        )}
      </div>
    </div>
  );
}
