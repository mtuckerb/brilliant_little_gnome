import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { ContentItem } from "../types";
import { topicViewUrl } from "../lib/brightspace";
import { watchZoteroLinks } from "../lib/zotero";

function sourceKey(raw: string, host: string | null): string | null {
  try {
    const value = host && raw.startsWith(`${host}/`) ? `https://${raw}` : raw;
    const url = host ? new URL(value, `https://${host}/`) : new URL(value);
    if (!["http:", "https:"].includes(url.protocol)) return null;
    url.hash = "";
    return url.href;
  } catch {
    return null;
  }
}

/** Resolve only known course sources; task notes themselves remain editable originals. */
export function zoteroSourceResolver(items: ContentItem[], host: string | null, courseId: string) {
  const destinations = new Map<string, string | null>();
  for (const item of items) {
    if (!item.zotero_url) continue;
    const sources = [item.url, host ? topicViewUrl(host, courseId, item.brightspace_id) : null];
    for (const source of sources) {
      const key = source ? sourceKey(source, host) : null;
      if (!key) continue;
      const previous = destinations.get(key);
      // A shared URL can refer to distinct Zotero entries. Do not guess.
      destinations.set(key, previous === undefined || previous === item.zotero_url ? item.zotero_url : null);
    }
  }
  return (url: string): string => {
    const key = sourceKey(url, host);
    return (key && destinations.get(key)) || url;
  };
}

export function useZoteroSourceLinks(courseId: string | undefined, host: string | null) {
  const [snapshot, setSnapshot] = useState<{ courseId: string; items: ContentItem[] } | null>(null);
  useEffect(() => {
    if (!courseId) return;
    let active = true;
    let revision = 0;
    const refresh = () => {
      const current = ++revision;
      api.listCourseItems(courseId).then((items) => {
        if (active && current === revision) setSnapshot({ courseId, items });
      }).catch(() => {
        if (active && current === revision) setSnapshot(null);
      });
    };
    const stop = watchZoteroLinks(refresh);
    return () => { active = false; stop(); };
  }, [courseId]);
  return useMemo(() => zoteroSourceResolver(
    snapshot && snapshot.courseId === courseId ? snapshot.items : [], host, courseId ?? "",
  ), [snapshot, courseId, host]);
}
