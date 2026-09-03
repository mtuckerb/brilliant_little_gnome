import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api } from "../api";
import type { ContentItem, ContentModule } from "../types";
import { triggerDownload } from "../lib/download";
import HeaderBand from "../components/HeaderBand";
import { useBrightspaceHost } from "../components/BrightspaceLink";
import { moduleUrl, topicViewUrl } from "../lib/brightspace";
import SyntheticTaskModal from "../components/SyntheticTaskModal";
import { useToast } from "../components/ToastProvider";
import { mdEscape, mdLink } from "../lib/markdown";
import { moduleTaskDue } from "../lib/moduleTasks";
import type { Course } from "../types";

// Course content as one tree: sub-modules are branches, items are leaves, and
// nothing is hidden behind a click-through. The old page listed modules only,
// so an item three levels down was three navigations away from being seen —
// easy to miss entirely. Everything is expanded by default; what you collapse
// is remembered per course, which means content added later shows up open
// rather than tucked inside a fold you closed months ago.

// Files open in the in-app viewer. Everything else (quizzes, LTI launches,
// discussions, outside links) only exists server-side, so it opens in the
// browser against your Brightspace session.
type OpenMode = "viewer" | "external";

interface Leaf {
  icon: string;
  mode: OpenMode;
  label: string | null;
}

function quickLinkType(url: string | null): string | null {
  const m = url ? /[?&]type=([a-z]+)/i.exec(url) : null;
  return m ? m[1].toLowerCase() : null;
}

function describeItem(it: ContentItem): Leaf {
  const type = it.item_type?.toLowerCase();
  // A File topic is fetched through the topic endpoint, so it opens in the
  // viewer whether or not Brightspace recorded a URL for it.
  if (type === "file" || (!type && !it.url)) {
    const ext = (/\.([a-z0-9]+)$/i.exec(it.url ?? "")?.[1] ?? "").toLowerCase();
    const icon =
      ext === "pdf" ? "fa-file-pdf"
      : ext === "doc" || ext === "docx" ? "fa-file-word"
      : ext === "ppt" || ext === "pptx" ? "fa-file-powerpoint"
      : ext === "xls" || ext === "xlsx" ? "fa-file-excel"
      : ext === "html" || ext === "htm" ? "fa-file-lines"
      : ext === "mp3" || ext === "mp4" || ext === "mov" ? "fa-file-video"
      : "fa-file";
    return { icon, mode: "viewer", label: ext ? ext.toUpperCase() : null };
  }

  const url = it.url ?? "";
  if (/open\.spotify\.com/i.test(url)) return { icon: "fa-spotify", mode: "external", label: "Playlist" };

  switch (quickLinkType(url)) {
    case "quiz": return { icon: "fa-circle-question", mode: "external", label: "Quiz" };
    case "discuss": return { icon: "fa-comments", mode: "external", label: "Discussion" };
    case "dropbox": return { icon: "fa-inbox", mode: "external", label: "Assignment" };
    case "lti": return { icon: "fa-puzzle-piece", mode: "external", label: "Tool" };
    case "survey": return { icon: "fa-square-poll-vertical", mode: "external", label: "Survey" };
    default: return { icon: "fa-link", mode: "external", label: "Link" };
  }
}

export default function Modules() {
  const { id: courseId } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const bsHost = useBrightspaceHost();
  const [modules, setModules] = useState<ContentModule[] | null>(null);
  const [items, setItems] = useState<ContentItem[] | null>(null);
  const [course, setCourse] = useState<Course | null>(null);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [downloading, setDownloading] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState<{
    name: string;
    description: string;
    due?: string;
  } | null>(null);
  const toast = useToast();

  const foldKey = `modules.collapsed:${courseId}`;

  useEffect(() => {
    if (!courseId) return;
    setModules(null);
    setItems(null);
    setCourse(null);
    api.listModules(courseId).then(setModules);
    api.listCourseItems(courseId).then(setItems);
    api.getCourse(courseId).then(setCourse);
    try {
      const saved = localStorage.getItem(`modules.collapsed:${courseId}`);
      setCollapsed(new Set(saved ? (JSON.parse(saved) as string[]) : []));
    } catch {
      setCollapsed(new Set());
    }
  }, [courseId]);

  const remember = useCallback(
    (next: Set<string>) => {
      try {
        localStorage.setItem(foldKey, JSON.stringify([...next]));
      } catch {
        /* a fold preference is not worth surfacing a quota error for */
      }
    },
    [foldKey],
  );

  const toggle = useCallback(
    (id: string) => {
      setCollapsed((prev) => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        remember(next);
        return next;
      });
    },
    [remember],
  );

  const childrenOf = useMemo(() => {
    const map = new Map<string, ContentModule[]>();
    for (const m of modules ?? []) {
      const key = m.parent_id ?? "";
      const list = map.get(key);
      if (list) list.push(m);
      else map.set(key, [m]);
    }
    return map;
  }, [modules]);

  const itemsOf = useMemo(() => {
    const map = new Map<string, ContentItem[]>();
    for (const it of items ?? []) {
      const list = map.get(it.module_id);
      if (list) list.push(it);
      else map.set(it.module_id, [it]);
    }
    return map;
  }, [items]);

  // A module's parent may be missing from this course's rows (unenrolled, or
  // simply never synced), which would strand its whole subtree. Anything whose
  // parent isn't here is treated as a root so it still renders.
  const roots = useMemo(() => {
    const known = new Set((modules ?? []).map((m) => m.brightspace_id));
    return (modules ?? []).filter((m) => !m.parent_id || !known.has(m.parent_id));
  }, [modules]);

  const q = query.trim().toLowerCase();
  const hit = useCallback((s: string) => s.toLowerCase().includes(q), [q]);

  // A module survives the filter if it matches, or if anything beneath it
  // does — otherwise a deep match would be stranded behind a hidden parent.
  const matches = useCallback(
    (m: ContentModule, seen = new Set<string>()): boolean => {
      if (!q) return true;
      if (seen.has(m.brightspace_id)) return false;
      seen.add(m.brightspace_id);
      if (hit(m.title)) return true;
      if ((itemsOf.get(m.brightspace_id) ?? []).some((it) => hit(it.title))) return true;
      return (childrenOf.get(m.brightspace_id) ?? []).some((c) => matches(c, seen));
    },
    [q, hit, itemsOf, childrenOf],
  );

  const countBelow = useCallback(
    (m: ContentModule, seen = new Set<string>()): number => {
      if (seen.has(m.brightspace_id)) return 0;
      seen.add(m.brightspace_id);
      return (
        (itemsOf.get(m.brightspace_id) ?? []).length +
        (childrenOf.get(m.brightspace_id) ?? []).reduce((n, c) => n + countBelow(c, seen), 0)
      );
    },
    [itemsOf, childrenOf],
  );

  async function openItem(it: ContentItem) {
    if (!courseId) return;
    const leaf = describeItem(it);
    if (leaf.mode === "viewer") {
      navigate("/viewer", {
        state: { courseId, topicId: it.brightspace_id, name: it.url ?? it.title },
      });
      return;
    }
    const raw = it.url ?? (bsHost ? topicViewUrl(bsHost, courseId, it.brightspace_id) : null);
    if (!raw) {
      setError(`${it.title}: no link recorded for this item.`);
      return;
    }
    const url = raw.startsWith("/") && bsHost ? `https://${bsHost}${raw}` : raw;
    await api.openUrl(url);
  }

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

  function moduleYear(): number {
    const semester = course?.custom_semester || course?.semester || "";
    return Number(/\b(20\d{2})\b/.exec(semester)?.[1] ?? new Date().getFullYear());
  }

  function itemUrl(it: ContentItem): string | null {
    const raw = it.url ?? (bsHost && courseId ? topicViewUrl(bsHost, courseId, it.brightspace_id) : null);
    if (!raw) return null;
    return raw.startsWith("/") && bsHost ? `https://${bsHost}${raw}` : raw;
  }

  function taskFromItem(it: ContentItem) {
    const parent = modules?.find((m) => m.brightspace_id === it.module_id);
    const where = parent?.title ? `From module **${mdEscape(parent.title)}**` : "From a course module";
    const link = itemUrl(it);
    const linkLine = link ? `\n\n${mdLink(it.title, link)}` : "";
    return {
      name: it.title,
      description: `${where} — ${mdEscape(it.title)}${linkLine}`,
      due: moduleTaskDue(it.module_id, modules ?? [], course?.end_of_week_day ?? null, moduleYear()),
    };
  }

  function taskFromModule(m: ContentModule) {
    const link = bsHost && courseId ? moduleUrl(bsHost, courseId, m.brightspace_id) : null;
    const linkLine = link ? `\n\n${mdLink(m.title, link)}` : "";
    return {
      name: m.title,
      description: `From course content — module **${mdEscape(m.title)}**${linkLine}`,
      due: moduleTaskDue(m.brightspace_id, modules ?? [], course?.end_of_week_day ?? null, moduleYear()),
    };
  }

  if (!modules || !items || !course) {
    return (
      <div className="has-text-centered py-6">
        <span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span>
      </div>
    );
  }

  function renderModule(m: ContentModule, seen: Set<string>): JSX.Element | null {
    if (seen.has(m.brightspace_id) || !matches(m)) return null;
    const branch = new Set(seen).add(m.brightspace_id);

    const kids = (childrenOf.get(m.brightspace_id) ?? []).filter((c) => matches(c));
    // When the module itself matches, keep all its items; otherwise show only
    // the ones that matched.
    const own = (itemsOf.get(m.brightspace_id) ?? []).filter((it) => !q || hit(m.title) || hit(it.title));
    const open = q ? true : !collapsed.has(m.brightspace_id); // a search result is never worth folding away
    const total = countBelow(m);
    const zipKey = `mod:${m.id}`;
    const hasBody = kids.length > 0 || own.length > 0;

    return (
      <div key={m.id}>
        <div className="is-flex is-align-items-center py-1" style={{ gap: 6 }}>
          <button
            className="button is-white is-small px-1"
            style={{ height: 24, visibility: hasBody ? "visible" : "hidden" }}
            aria-label={open ? `Collapse ${m.title}` : `Expand ${m.title}`}
            aria-expanded={open}
            onClick={() => toggle(m.brightspace_id)}
          >
            <span className="icon is-small has-text-grey">
              <i className={`fas ${open ? "fa-chevron-down" : "fa-chevron-right"}`}></i>
            </span>
          </button>
          <span className="icon is-small has-text-grey-light">
            <i className={`fas ${open && hasBody ? "fa-folder-open" : "fa-folder"}`}></i>
          </span>
          <button
            className="button is-white is-small px-0"
            style={{ height: 24, fontWeight: 600, textAlign: "left", whiteSpace: "normal" }}
            onClick={() => toggle(m.brightspace_id)}
          >
            {m.title}
          </button>
          <button
            className="button is-white is-small px-1"
            title="Create a task from this module"
            aria-label={`Create a task from ${m.title}`}
            onClick={() => setCreating(taskFromModule(m))}
          >
            <span className="icon is-small has-text-grey-light"><i className="fas fa-list-check"></i></span>
          </button>
          {total > 0 && <span className="tag is-light is-rounded is-size-7">{total}</span>}
          <Link
            to={`/course/${courseId}/content/${m.brightspace_id}`}
            className="icon is-small has-text-grey-light"
            title="Module page — instructor notes, Zotero, bulk actions"
          >
            <i className="fas fa-circle-info"></i>
          </Link>
          <button
            className="button is-small is-white ml-auto"
            title="Download this module's files as a ZIP"
            disabled={downloading[zipKey] || total === 0}
            onClick={() => downloadModule(m)}
          >
            <span className="icon is-small has-text-grey">
              <i className={`fas ${downloading[zipKey] ? "fa-circle-notch fa-spin" : "fa-file-archive"}`}></i>
            </span>
          </button>
        </div>

        {open && hasBody && (
          <div style={{ marginLeft: 18, borderLeft: "1px solid #ececec", paddingLeft: 10 }}>
            {kids.map((c) => renderModule(c, branch))}
            {own.map((it) => {
              const leaf = describeItem(it);
              return (
                <div key={it.id} className="is-flex is-align-items-center py-1" style={{ gap: 6 }}>
                  <span className="icon is-small has-text-grey-light" style={{ marginLeft: 24 }}>
                    <i className={`${leaf.icon === "fa-spotify" ? "fab" : "fas"} ${leaf.icon}`}></i>
                  </span>
                  <button
                    className="button is-white is-small px-0 has-text-link"
                    style={{ height: "auto", minHeight: 24, textAlign: "left", whiteSpace: "normal" }}
                    onClick={() => openItem(it)}
                    title={leaf.mode === "viewer" ? "Open in Brilliant" : "Open in Brightspace"}
                  >
                    {it.title}
                  </button>
                  <button
                    className="button is-white is-small px-1"
                    title="Create a task from this item"
                    aria-label={`Create a task from ${it.title}`}
                    onClick={() => setCreating(taskFromItem(it))}
                  >
                    <span className="icon is-small has-text-grey-light"><i className="fas fa-list-check"></i></span>
                  </button>
                  {leaf.label && <span className="tag is-light is-size-7">{leaf.label}</span>}
                  {it.is_hidden && <span className="tag is-light is-size-7">hidden</span>}
                  {leaf.mode === "external" && (
                    <span className="icon is-small has-text-grey-lighter">
                      <i className="fas fa-arrow-up-right-from-square"></i>
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    );
  }

  const visible = roots.map((m) => renderModule(m, new Set())).filter(Boolean);

  return (
    <div>
      {courseId && <HeaderBand courseId={courseId} />}
      <div className="level mb-3">
        <div className="level-left">
          <h1 className="title mb-0"><i className="fas fa-folder-tree mr-2"></i>Course content</h1>
        </div>
        <div className="level-right">
          <div className="field has-addons mb-0">
            <div className="control has-icons-left">
              <input
                className="input is-small"
                placeholder="Filter content…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
              <span className="icon is-small is-left"><i className="fas fa-magnifying-glass"></i></span>
            </div>
            <div className="control">
              <button
                className="button is-small"
                title="Collapse every module"
                onClick={() => {
                  const all = new Set(modules.map((m) => m.brightspace_id));
                  setCollapsed(all);
                  remember(all);
                }}
              >
                <span className="icon is-small"><i className="fas fa-compress"></i></span>
              </button>
            </div>
            <div className="control">
              <button
                className="button is-small"
                title="Expand everything"
                onClick={() => {
                  setCollapsed(new Set());
                  remember(new Set());
                }}
              >
                <span className="icon is-small"><i className="fas fa-expand"></i></span>
              </button>
            </div>
          </div>
        </div>
      </div>

      {error && <div className="notification is-danger is-light is-size-7">{error}</div>}

      <div className="box">
        {modules.length === 0 ? (
          <p className="has-text-grey">No modules yet — try syncing.</p>
        ) : visible.length === 0 ? (
          <p className="has-text-grey is-size-7">Nothing matches “{query}”.</p>
        ) : (
          visible
        )}
      </div>

      {courseId && (
        <SyntheticTaskModal
          courseId={courseId}
          open={creating !== null}
          onClose={() => setCreating(null)}
          onCreated={(assignment) => {
            setCreating(null);
            toast.show(`Task "${assignment.name}" created.`, "is-success");
          }}
          initialName={creating?.name}
          initialDescription={creating?.description}
          initialDue={creating?.due}
        />
      )}
    </div>
  );
}
