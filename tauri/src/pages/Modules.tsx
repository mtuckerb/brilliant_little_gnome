import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { ContentModule } from "../types";
import { triggerDownload } from "../lib/download";
import CourseNav from "../components/CourseNav";

// Modules listing — clicking a module navigates to its detail page where
// instructor commentary and downloadable items live. Child modules nest
// visually below their parent so a course's structure is still scannable
// at a glance.

export default function Modules() {
  const { id: courseId } = useParams<{ id: string }>();
  const [modules, setModules] = useState<ContentModule[] | null>(null);
  const [downloading, setDownloading] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!courseId) return;
    api.listModules(courseId).then(setModules);
  }, [courseId]);

  async function downloadModule(m: ContentModule) {
    if (!courseId) return;
    const key = `mod:${m.id}`;
    setDownloading((s) => ({ ...s, [key]: true }));
    setError(null);
    try {
      const payload = await api.downloadModuleArchive(courseId, m.brightspace_id);
      triggerDownload(payload);
    } catch (e) {
      setError(`${m.title}: ${String((e as { message?: string })?.message ?? e)}`);
    } finally {
      setDownloading((s) => ({ ...s, [key]: false }));
    }
  }

  if (!modules) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  const tops = modules.filter((m) => !m.parent_id);
  const childrenOf = (pid: string) => modules.filter((m) => m.parent_id === pid);

  const renderModule = (m: ContentModule, depth = 0): JSX.Element => (
    <div key={m.id} style={{ marginLeft: depth * 20 }} className="mb-2">
      <div className="is-flex is-align-items-center" style={{ gap: 8 }}>
        <span className="icon has-text-grey"><i className="fas fa-folder"></i></span>
        <Link to={`/course/${courseId}/content/${m.brightspace_id}`} className="has-text-link">
          <strong>{m.title}</strong>
        </Link>
        <button
          className="button is-small is-light ml-auto"
          title="Download all files in this module as a ZIP"
          disabled={downloading[`mod:${m.id}`]}
          onClick={() => downloadModule(m)}
        >
          <span className="icon is-small">
            <i className={`fas ${downloading[`mod:${m.id}`] ? "fa-circle-notch fa-spin" : "fa-file-archive"}`}></i>
          </span>
        </button>
      </div>
      {childrenOf(m.brightspace_id).length > 0 && (
        <div className="ml-4 mt-1">
          {childrenOf(m.brightspace_id).map((c) => renderModule(c, depth + 1))}
        </div>
      )}
    </div>
  );

  return (
    <div>
      {courseId && <CourseNav courseId={courseId} />}
      <h1 className="title"><i className="fas fa-folder-tree mr-2"></i>Course content</h1>
      {error && <div className="notification is-danger is-light is-size-7">{error}</div>}
      <div className="box">
        {tops.length === 0 && <p className="has-text-grey">No modules yet — try syncing.</p>}
        {tops.map((m) => renderModule(m))}
      </div>
    </div>
  );
}
