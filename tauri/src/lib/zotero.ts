// Shared helper for "Send to Zotero" buttons.
//
// Wraps a single Zotero API call with toast-based feedback so each call site
// (SyllabusPanel / CourseDetail / ModuleDetail) doesn't have to reinvent the
// success / failure messaging. The Zotero work happens in Rust; we just
// surface the result here.

import type { ZoteroResult } from "../api";

interface ToastApi {
  show: (message: string, kind?: "is-info" | "is-success" | "is-warning" | "is-danger", duration?: number) => void;
}

export async function runZotero(
  toast: ToastApi,
  label: string,
  action: () => Promise<ZoteroResult>,
): Promise<ZoteroResult | null> {
  toast.show(`Sending ${label} to Zotero…`, "is-info", 3000);
  try {
    const result = await action();
    const created = result.created.length;
    const failed = result.failures.length;
    // A re-send of an unchanged course does nothing at all, which is success.
    // Reporting it as "nothing to send" made a working sync read as a dead
    // end, so up-to-date items are counted and called out separately.
    const current = result.up_to_date ?? 0;
    const alsoCurrent = current > 0 ? `, ${current} already current` : "";
    // Relations are best-effort in the backend, so a library that rejects
    // every one of them still comes back with no failures. Staying silent
    // here is how a server 500ing on every relation write went on reading
    // as an unqualified success.
    const unlinked = result.relation_failures?.length ?? 0;
    const linkNote =
      unlinked > 0
        ? ` ${unlinked} module${unlinked === 1 ? "" : "s"} could not be cross-linked under Related — first: ${escapeHtml(result.relation_failures[0])}`
        : "";

    let message: string;
    let kind: "is-success" | "is-warning" | "is-danger";
    if (created === 0 && failed === 0 && current > 0) {
      message = `${label}: already up to date — ${current} item${current === 1 ? "" : "s"} in Zotero.`;
      kind = "is-success";
    } else if (created === 0 && failed === 0) {
      message = `${label}: nothing to send.`;
      kind = "is-warning";
    } else if (failed === 0) {
      message = `${label}: sent ${created} item${created === 1 ? "" : "s"} to Zotero${alsoCurrent}.`;
      kind = "is-success";
    } else if (created === 0) {
      message = `${label}: send failed. First error: ${escapeHtml(result.failures[0])}`;
      kind = "is-danger";
    } else {
      message = `${label}: sent ${created}${alsoCurrent}, ${failed} failed. First error: ${escapeHtml(result.failures[0])}`;
      kind = "is-warning";
    }

    // The documents arrived either way, so unlinked modules never turn a
    // send red — but they do stop it claiming to be clean.
    if (unlinked > 0 && kind === "is-success") {
      kind = "is-warning";
    }
    toast.show(message + linkNote, kind, kind === "is-success" ? undefined : 9000);
    return result;
  } catch (e) {
    toast.show(
      `${label}: ${escapeHtml(String((e as { message?: string })?.message ?? e))}`,
      "is-danger",
      9000,
    );
    return null;
  }
}

function escapeHtml(s: string): string {
  return s
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}
