import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useParams, useSearchParams } from "react-router-dom";
import { api } from "../api";
import type { Assignment, ContentItem, ContentModule, DiscussionTopic } from "../types";
import HeaderBand from "../components/HeaderBand";

// Per-course search. Reuses the data the app has already synced: assignments,
// modules + items, discussion topics. Filters client-side by case-insensitive
// substring on titles/names — enough to find "Reading 12" or "Reflection #3"
// without a real search engine. Results are grouped by kind so the user can
// jump straight to the corresponding detail page.

type Kind = "assignment" | "module" | "item" | "topic";

interface Result {
  kind: Kind;
  title: string;
  href: string;
  hint?: string;
}

function fuzzy(needle: string, hay: string): boolean {
  return hay.toLowerCase().includes(needle.toLowerCase());
}

export default function CourseSearch() {
  const { id: courseId } = useParams<{ id: string }>();
  const [params, setParams] = useSearchParams();
  const initialQ = params.get("q") ?? "";
  const [q, setQ] = useState(initialQ);
  const [assignments, setAssignments] = useState<Assignment[] | null>(null);
  const [modules, setModules] = useState<ContentModule[] | null>(null);
  const [items, setItems] = useState<ContentItem[]>([]);
  const [topics, setTopics] = useState<DiscussionTopic[] | null>(null);

  const load = useCallback(async () => {
    if (!courseId) return;
    const [a, m, t] = await Promise.all([
      api.listAssignments(courseId, undefined).catch(() => []),
      api.listModules(courseId).catch(() => []),
      api.listTopics(courseId).catch(() => []),
    ]);
    setAssignments(a);
    setModules(m);
    setTopics(t);
    // Items are per-module — fetch in parallel for everything.
    const allItems = await Promise.all(
      m.map((mod) => api.listItems(mod.brightspace_id).catch(() => [] as ContentItem[])),
    );
    setItems(allItems.flat());
  }, [courseId]);

  useEffect(() => { load(); }, [load]);

  // Keep the URL ?q= in sync so the search is bookmarkable / back-button-safe.
  useEffect(() => {
    if (q === (params.get("q") ?? "")) return;
    const next = new URLSearchParams(params);
    if (q) next.set("q", q); else next.delete("q");
    setParams(next, { replace: true });
  }, [q]); // eslint-disable-line react-hooks/exhaustive-deps

  const results = useMemo<Result[]>(() => {
    if (!q.trim() || !courseId) return [];
    const out: Result[] = [];
    const moduleById: Record<string, ContentModule> = {};
    (modules ?? []).forEach((m) => { moduleById[m.brightspace_id] = m; });

    (assignments ?? []).filter((a) => fuzzy(q, a.name)).forEach((a) =>
      out.push({
        kind: "assignment",
        title: a.name,
        href: `/course/${courseId}/assignments/${a.id}`,
        hint: a.due_date ? `Due ${new Date(a.due_date).toLocaleDateString()}` : undefined,
      }),
    );
    (modules ?? []).filter((m) => fuzzy(q, m.title)).forEach((m) =>
      out.push({
        kind: "module",
        title: m.title,
        href: `/course/${courseId}/content/${m.brightspace_id}`,
      }),
    );
    items.filter((it) => fuzzy(q, it.title)).forEach((it) => {
      const parent = moduleById[it.module_id];
      out.push({
        kind: "item",
        title: it.title,
        href: `/course/${courseId}/content/${it.module_id}`,
        hint: parent ? `in ${parent.title}` : undefined,
      });
    });
    (topics ?? []).filter((t) => fuzzy(q, t.name)).forEach((t) =>
      out.push({
        kind: "topic",
        title: t.name,
        href: `/course/${courseId}/discussions/${t.brightspace_id}`,
      }),
    );
    return out;
  }, [q, courseId, assignments, modules, items, topics]);

  const byKind: Record<Kind, Result[]> = { assignment: [], module: [], item: [], topic: [] };
  results.forEach((r) => byKind[r.kind].push(r));

  const KIND_META: Record<Kind, { label: string; icon: string }> = {
    assignment: { label: "Assignments", icon: "fa-tasks" },
    module: { label: "Modules", icon: "fa-folder-tree" },
    item: { label: "Content items", icon: "fa-file" },
    topic: { label: "Discussion topics", icon: "fa-comments" },
  };

  const loaded = assignments !== null && modules !== null && topics !== null;

  return (
    <div>
      {courseId && <HeaderBand courseId={courseId} />}
      <div className="box">
        <h2 className="title is-5 mb-3">
          <i className="fas fa-magnifying-glass mr-2 has-text-grey"></i>Search this course
        </h2>
        <input
          className="input"
          autoFocus
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="Find an assignment, module, item, or topic…"
        />
        {!loaded && <p className="has-text-grey is-size-7 mt-3">Loading…</p>}
        {loaded && q.trim() !== "" && results.length === 0 && (
          <p className="has-text-grey is-size-7 mt-3">No matches.</p>
        )}
        {(["assignment", "module", "item", "topic"] as Kind[]).map((kind) => {
          const list = byKind[kind];
          if (list.length === 0) return null;
          const meta = KIND_META[kind];
          return (
            <div key={kind} className="mt-4">
              <h3 className="is-size-7 has-text-weight-bold has-text-grey mb-2" style={{ textTransform: "uppercase", letterSpacing: 1 }}>
                <i className={`fas ${meta.icon} mr-2`}></i>{meta.label} ({list.length})
              </h3>
              <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
                {list.map((r, ix) => (
                  <li
                    key={`${r.kind}-${ix}-${r.href}`}
                    style={{ padding: "8px 0", borderTop: "1px solid #f0f0f0" }}
                  >
                    <Link to={r.href} className="has-text-link-dark">{r.title}</Link>
                    {r.hint && <span className="has-text-grey is-size-7 ml-2">· {r.hint}</span>}
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
      </div>
    </div>
  );
}
