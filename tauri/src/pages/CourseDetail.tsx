import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import { displayCourseName, type Course } from "../types";
import SyllabusPanel from "../components/SyllabusPanel";
import { triggerDownload } from "../lib/download";
import CourseNav from "../components/CourseNav";

const DAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

export default function CourseDetail() {
  const { id } = useParams<{ id: string }>();
  const [course, setCourse] = useState<Course | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState("");

  useEffect(() => {
    if (!id) return;
    api.getCourse(id).then(setCourse).catch((e) => setErr(String(e?.message ?? e)));
  }, [id]);

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

  async function onTitleSave() {
    if (!course) return;
    const previous = course;
    setCourse({ ...course, custom_name: titleDraft.trim() === "" ? null : titleDraft.trim() });
    setEditingTitle(false);
    try {
      const updated = await api.updateCourseName(course.org_unit_id, titleDraft);
      setCourse(updated);
    } catch (e) {
      setCourse(previous);
      alert(`Title save failed: ${String((e as { message?: string })?.message ?? e)}`);
    }
  }

  async function onDownloadEverything() {
    if (!course || downloading) return;
    if (!confirm("Download every file in this course? This may take a while for large courses.")) return;
    setDownloading(true);
    try {
      const payload = await api.downloadCourseArchive(course.org_unit_id);
      triggerDownload(payload);
    } catch (e) {
      alert(`Download failed: ${String((e as { message?: string })?.message ?? e)}`);
    } finally {
      setDownloading(false);
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

  const accent = course.custom_color || "#739AC3";
  const banner = course.banner_url;
  const courseTitle = displayCourseName(course);

  return (
    <div>
      <nav className="breadcrumb mb-3">
        <ul>
          <li><Link to="/dashboard">Dashboard</Link></li>
          <li className="is-active"><a>{courseTitle}</a></li>
        </ul>
      </nav>
      <CourseNav courseId={course.org_unit_id} />

      <section
        className="hero is-small mb-5"
        style={{
          borderRadius: 12,
          overflow: "hidden",
          position: "relative",
          backgroundImage: banner
            ? `url(${banner})`
            : "linear-gradient(135deg, #2f4f6f 0%, #739AC3 100%)",
          backgroundSize: "cover",
          backgroundPosition: "center",
          border: `1px solid ${accent}`,
          boxShadow: "0 4px 15px rgba(0,0,0,0.1)",
        }}
      >
        <div
          className="hero-body"
          style={{
            background:
              "linear-gradient(to right, rgba(0,0,0,0.8) 0%, rgba(0,0,0,0.3) 50%, rgba(0,0,0,0.1) 100%)",
            padding: "3rem 2rem",
          }}
        >
          {editingTitle ? (
            <div className="field has-addons" style={{ maxWidth: 900 }}>
              <div className="control is-expanded">
                <input
                  className="input is-large"
                  value={titleDraft}
                  onChange={(e) => setTitleDraft(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter") onTitleSave(); if (e.key === "Escape") setEditingTitle(false); }}
                  autoFocus
                />
              </div>
              <div className="control"><button className="button is-light is-large" onClick={onTitleSave}>Save</button></div>
              <div className="control"><button className="button is-dark is-large" onClick={() => setEditingTitle(false)}>Cancel</button></div>
            </div>
          ) : (
            <h1
              className="title is-2 has-text-white is-flex is-align-items-center"
              style={{ textShadow: "0 2px 10px rgba(0,0,0,0.8)", lineHeight: 1.1, gap: 10 }}
            >
              <span>{courseTitle}</span>
              <button
                className="button is-small is-light is-outlined"
                title="Edit course title"
                onClick={() => { setTitleDraft(courseTitle); setEditingTitle(true); }}
              >
                <span className="icon"><i className="fas fa-pencil-alt"></i></span>
              </button>
            </h1>
          )}
          {course.code && (
            <p
              className="is-size-7 has-text-white is-uppercase"
              style={{
                letterSpacing: 1.5,
                textShadow: "0 1px 2px rgba(0,0,0,0.5)",
                position: "absolute",
                bottom: 10,
                right: 20,
                opacity: 0.75,
              }}
            >
              {course.code}
            </p>
          )}
        </div>
      </section>

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
      </div>

      <SyllabusPanel courseId={course.org_unit_id} />

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
    </div>
  );
}
