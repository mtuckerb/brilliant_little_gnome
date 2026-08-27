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
    if (created === 0 && failed === 0 && current > 0) {
      toast.show(
        `${label}: already up to date — ${current} item${current === 1 ? "" : "s"} in Zotero.`,
        "is-success",
      );
    } else if (created === 0 && failed === 0) {
      toast.show(`${label}: nothing to send.`, "is-warning");
    } else if (failed === 0) {
      toast.show(
        `${label}: sent ${created} item${created === 1 ? "" : "s"} to Zotero${alsoCurrent}.`,
        "is-success",
      );
    } else if (created === 0) {
      toast.show(
        `${label}: send failed. First error: ${escapeHtml(result.failures[0])}`,
        "is-danger",
        9000,
      );
    } else {
      toast.show(
        `${label}: sent ${created}${alsoCurrent}, ${failed} failed. First error: ${escapeHtml(result.failures[0])}`,
        "is-warning",
        9000,
      );
    }
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
