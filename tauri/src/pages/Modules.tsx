import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { api } from "../api";
import type { ContentModule, ContentItem } from "../types";

export default function Modules() {
  const { id: courseId } = useParams<{ id: string }>();
  const [modules, setModules] = useState<ContentModule[] | null>(null);
  const [itemsByModule, setItemsByModule] = useState<Record<string, ContentItem[]>>({});
  const [open, setOpen] = useState<Record<string, boolean>>({});

  useEffect(() => {
    if (!courseId) return;
    api.listModules(courseId).then(setModules);
  }, [courseId]);

  async function toggle(m: ContentModule) {
    setOpen((s) => ({ ...s, [m.brightspace_id]: !s[m.brightspace_id] }));
    if (!itemsByModule[m.brightspace_id]) {
      const items = await api.listItems(m.brightspace_id);
      setItemsByModule((s) => ({ ...s, [m.brightspace_id]: items }));
    }
  }

  if (!modules) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  // Top-level modules first, then nested under their parents.
  const tops = modules.filter((m) => !m.parent_id);
  const childrenOf = (pid: string) => modules.filter((m) => m.parent_id === pid);

  const renderModule = (m: ContentModule, depth = 0): JSX.Element => (
    <div key={m.id} style={{ marginLeft: depth * 20 }} className="mb-2">
      <button className="button is-ghost is-small" onClick={() => toggle(m)}>
        <span className="icon"><i className={`fas fa-chevron-${open[m.brightspace_id] ? "down" : "right"}`}></i></span>
        <span><strong>{m.title}</strong></span>
      </button>
      {open[m.brightspace_id] && (
        <div className="ml-4 mt-1">
          {(itemsByModule[m.brightspace_id] || []).map((it) => (
            <div key={it.id} className="is-size-7 py-1">
              <span className="icon is-small"><i className="far fa-file"></i></span>
              {it.url ? <a href={it.url} target="_blank" rel="noopener noreferrer">{it.title}</a> : it.title}
              {it.is_hidden && <span className="tag is-light ml-2">hidden</span>}
            </div>
          ))}
          {childrenOf(m.brightspace_id).map((c) => renderModule(c, depth + 1))}
        </div>
      )}
    </div>
  );

  return (
    <div>
      <h1 className="title"><i className="fas fa-folder-tree mr-2"></i>Course content</h1>
      <div className="box">
        {tops.length === 0 && <p className="has-text-grey">No modules yet — try syncing.</p>}
        {tops.map((m) => renderModule(m))}
      </div>
    </div>
  );
}
