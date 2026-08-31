import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { ContentItem, ContentModule } from "../types";
import { triggerDownload } from "../lib/download";
import HeaderBand from "../components/HeaderBand";
import { runZotero } from "../lib/zotero";
import { useToast } from "../components/ToastProvider";
import BrightspaceLink, { useBrightspaceHost } from "../components/BrightspaceLink";
import { moduleUrl, topicViewUrl } from "../lib/brightspace";
import RichText from "../components/RichText";
import SyntheticTaskModal from "../components/SyntheticTaskModal";

// Module detail — shows the module's instructor commentary (description),
// child sub-modules as clickable links, and the list of items inside the
// module with per-file + bulk download. The tree-toggle expand/collapse
// from the old Modules page is replaced by direct navigation so you get a
// dedicated URL per module.

export default function ModuleDetail() {
  const { id: courseId, moduleId } = useParams<{ id: string; moduleId: string }>();
  const [modules, setModules] = useState<ContentModule[] | null>(null);
  const [items, setItems] = useState<ContentItem[] | null>(null);
  const [downloading, setDownloading] = useState<Record<string, boolean>>({});
  const [moduleZipping, setModuleZipping] = useState(false);
  const [zoteroBusy, setZoteroBusy] = useState<Record<string, boolean>>({});
  const [moduleSendingZotero, setModuleSendingZotero] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Prefill for the task modal, set by the "make a task from this" action on
  // any row — content items and sub-modules both feed it.
  const [creating, setCreating] = useState<{ name: string; description: string } | null>(null);
  const toast = useToast();
  const bsHost = useBrightspaceHost();

  useEffect(() => {
    if (!courseId || !moduleId) return;
    api.listModules(courseId).then(setModules);
    api.listItems(moduleId).then(setItems);
  }, [courseId, moduleId]);

  const current = useMemo(
    () => modules?.find((m) => m.brightspace_id === moduleId) ?? null,
    [modules, moduleId],
  );
  const children = useMemo(
    () => (modules ?? []).filter((m) => m.parent_id === moduleId),
    [modules, moduleId],
  );
  const parent = useMemo(() => {
    if (!current?.parent_id || !modules) return null;
    return modules.find((m) => m.brightspace_id === current.parent_id) ?? null;
  }, [current, modules]);

  function isDownloadableFile(it: ContentItem) {
    const type = it.item_type?.toLowerCase();
    if (type) return type === "file";
    return !it.url;
  }

  async function openItemInBrightspace(it: ContentItem) {
    if (!courseId) return;
    const url = it.url ?? (bsHost ? topicViewUrl(bsHost, courseId, it.brightspace_id) : null);
    if (!url) {
      setError(`${it.title}: Brightspace URL is not available.`);
      return;
    }
    await api.openUrl(url);
  }

  async function downloadItem(it: ContentItem) {
    if (!courseId) return;
    if (!isDownloadableFile(it)) {
      await openItemInBrightspace(it);
      return;
    }
    const key = `item:${it.id}`;
    setDownloading((s) => ({ ...s, [key]: true }));
    setError(null);
    try {
      const payload = await api.downloadTopicFile(courseId, it.brightspace_id);
      triggerDownload(payload);
    } catch (e) {
      setError(`${it.title}: ${String((e as { message?: string })?.message ?? e)}`);
    } finally {
      setDownloading((s) => ({ ...s, [key]: false }));
    }
  }

  async function downloadModule() {
    if (!courseId || !moduleId) return;
    setModuleZipping(true);
    setError(null);
    try {
      const payload = await api.downloadModuleArchive(courseId, moduleId);
      triggerDownload(payload);
    } catch (e) {
      const msg = String((e as { message?: string })?.message ?? e);
      if (msg.toLowerCase().includes("authentication required")) {
        toast.show(
          "Your Brightspace session expired. Sign in again (status indicator, top right), then retry the download.",
          "is-danger",
          8000,
        );
        setError(null);
      } else {
        setError(`${current?.title ?? "module"}: ${msg}`);
      }
    } finally {
      setModuleZipping(false);
    }
  }

  // Turning a row into a task: the row's title is usually already the thing
  // you have to do ("Levenson, 2017TIP" → read it), so it seeds the name and
  // the notes carry the trail back to where it came from.
  function itemBrightspaceUrl(it: ContentItem): string | null {
    if (it.url) return it.url;
    if (bsHost && courseId) return topicViewUrl(bsHost, courseId, it.brightspace_id);
    return null;
  }

  function taskFromItem(it: ContentItem) {
    const where = current?.title ? `From module **${current.title}**` : "From a course module";
    const link = itemBrightspaceUrl(it);
    const linkLine = link ? `\n\n[${it.title}](${link})` : "";
    return { name: it.title, description: `${where} — ${it.title}${linkLine}` };
  }

  // A sub-module is a folder, so the task is "work through all of it" — the
  // note links to the sub-module page rather than a single file.
  function taskFromSubModule(c: ContentModule) {
    const where = current?.title ? `From module **${current.title}**` : "From a course module";
    const link = bsHost && courseId ? moduleUrl(bsHost, courseId, c.brightspace_id) : null;
    const linkLine = link ? `\n\n[${c.title}](${link})` : "";
    return {
      name: `${c.title} — everything in this section`,
      description: `${where} — sub-module **${c.title}**${linkLine}`,
    };
  }

  async function sendItemToZotero(it: ContentItem) {
    if (!courseId || !isDownloadableFile(it)) return;
    const key = `item:${it.id}`;
    setZoteroBusy((s) => ({ ...s, [key]: true }));
    await runZotero(toast, it.title, () =>
      api.zoteroSendTopic(courseId, it.brightspace_id),
    );
    setZoteroBusy((s) => ({ ...s, [key]: false }));
  }

  async function sendModuleToZotero() {
    if (!courseId || !moduleId) return;
    setModuleSendingZotero(true);
    await runZotero(toast, current?.title ?? "Module", () =>
      api.zoteroSendModule(courseId, moduleId),
    );
    setModuleSendingZotero(false);
  }

  if (modules === null || items === null) {
    return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;
  }

  if (!current) {
    return (
      <div>
        {courseId && <HeaderBand courseId={courseId} />}
        <div className="notification is-warning is-light">Module not found in this course.</div>
        <Link to={`/course/${courseId}/content`}>← back to modules</Link>
      </div>
    );
  }

  return (
    <div>
      {courseId && <HeaderBand courseId={courseId} />}

      <nav className="breadcrumb mb-3">
        <ul>
          <li><Link to={`/course/${courseId}`}>Course</Link></li>
          <li><Link to={`/course/${courseId}/content`}>Modules</Link></li>
          {parent && (
            <li>
              <Link to={`/course/${courseId}/content/${parent.brightspace_id}`}>{parent.title}</Link>
            </li>
          )}
          <li className="is-active"><a>{current.title}</a></li>
        </ul>
      </nav>

      <div className="level mb-3">
        <div className="level-left is-flex is-align-items-center">
          <h1 className="title is-4 mb-0"><i className="fas fa-folder-open mr-2"></i>{current.title}</h1>
          {bsHost && courseId && moduleId && (
            <BrightspaceLink
              url={moduleUrl(bsHost, courseId, moduleId)}
              label="Open this module in Brightspace"
              className="ml-2"
            />
          )}
        </div>
        <div className="level-right">
          <button
            className="button is-small is-light"
            disabled={moduleZipping || items.length === 0}
            onClick={downloadModule}
            title="Download all files in this module as a ZIP"
          >
            <span className="icon is-small">
              <i className={`fas ${moduleZipping ? "fa-circle-notch fa-spin" : "fa-file-archive"}`}></i>
            </span>
            <span>{moduleZipping ? "Bundling…" : "Download all"}</span>
          </button>
          <button
            className="button is-small is-light ml-2"
            disabled={moduleSendingZotero || items.length === 0}
            onClick={sendModuleToZotero}
            title="Send all files in this module (and sub-modules) to your Zotero library"
          >
            <span className="icon is-small">
              <i className={`fas ${moduleSendingZotero ? "fa-circle-notch fa-spin" : "fa-book-bookmark"}`}></i>
            </span>
            <span>{moduleSendingZotero ? "Sending…" : "Send to Zotero"}</span>
          </button>
        </div>
      </div>

      {error && <div className="notification is-danger is-light is-size-7">{error}</div>}

      {/* Instructor commentary / module description */}
      {current.description && (
        <div className="box">
          <h2 className="title is-6"><i className="fas fa-chalkboard-teacher mr-2 has-text-grey"></i>From the instructor</h2>
          <RichText content={current.description} />
        </div>
      )}

      {/* Sub-modules */}
      {children.length > 0 && (
        <div className="box">
          <h2 className="title is-6"><i className="fas fa-folder-tree mr-2 has-text-grey"></i>Sub-modules</h2>
          <ul>
            {children.map((c) => (
              <li key={c.id} className="py-1 is-flex is-align-items-center" style={{ gap: 8 }}>
                <Link to={`/course/${courseId}/content/${c.brightspace_id}`}>
                  <span className="icon is-small mr-1"><i className="fas fa-folder"></i></span>
                  {c.title}
                </Link>
                <button
                  className="button is-small is-white ml-auto"
                  title="Create a task covering this sub-module"
                  onClick={() => setCreating(taskFromSubModule(c))}
                >
                  <span className="icon is-small"><i className="fas fa-list-check"></i></span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Items / downloadable content */}
      <div className="box">
        <h2 className="title is-6"><i className="fas fa-file mr-2 has-text-grey"></i>Content</h2>
        {items.length === 0 ? (
          <p className="has-text-grey is-size-7">No items in this module.</p>
        ) : (
          <ul>
            {items.map((it) => {
              const downloadable = isDownloadableFile(it);
              const itemKey = `item:${it.id}`;
              return (
                <li key={it.id} className="py-2 is-flex is-align-items-center" style={{ gap: 8, borderTop: "1px solid #f0f0f0" }}>
                  <span className="icon is-small has-text-grey"><i className="far fa-file"></i></span>
                  {it.url ? (
                    <a href={it.url} target="_blank" rel="noopener noreferrer">{it.title}</a>
                  ) : (
                    <span>{it.title}</span>
                  )}
                  {it.item_type && <span className="tag is-light is-small">{it.item_type}</span>}
                  {it.is_hidden && <span className="tag is-light is-small">hidden</span>}
                  <button
                    className="button is-small is-white ml-auto"
                    title={downloadable ? "Download this file" : "Open in Brightspace"}
                    disabled={downloading[itemKey]}
                    onClick={() => downloadItem(it)}
                  >
                    <span className="icon is-small">
                      <i className={`fas ${downloading[itemKey] ? "fa-circle-notch fa-spin" : downloadable ? "fa-download" : "fa-up-right-from-square"}`}></i>
                    </span>
                  </button>
                  <button
                    className="button is-small is-white"
                    title={downloadable ? "Send this file to Zotero" : "Only downloadable files can be sent to Zotero"}
                    disabled={!downloadable || zoteroBusy[itemKey]}
                    onClick={() => sendItemToZotero(it)}
                  >
                    <span className="icon is-small">
                      <i className={`fas ${zoteroBusy[itemKey] ? "fa-circle-notch fa-spin" : "fa-book-bookmark"}`}></i>
                    </span>
                  </button>
                  <button
                    className="button is-small is-white"
                    title="Create a task from this item"
                    onClick={() => setCreating(taskFromItem(it))}
                  >
                    <span className="icon is-small"><i className="fas fa-list-check"></i></span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {courseId && (
        <SyntheticTaskModal
          courseId={courseId}
          open={creating !== null}
          onClose={() => setCreating(null)}
          onCreated={(a) => {
            setCreating(null);
            toast.show(`Task "${a.name}" created.`, "is-success");
          }}
          initialName={creating?.name}
          initialDescription={creating?.description}
        />
      )}
    </div>
  );
}
