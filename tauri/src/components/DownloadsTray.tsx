import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { DownloadResult } from "../lib/download";

// Floating tray that surfaces every successful download. Rust emits
// "download://saved" after writing the file to disk; this listens globally so
// it doesn't matter which page invoked the download.
//
// Items live in component state + sessionStorage so they survive route
// navigation (the tray is mounted at the App root) but clear when the app
// is closed — matches the mental model of "recent downloads from this run."

interface TrayItem {
  id: string;            // unique key for React
  filename: string;
  savedPath: string;
  mime: string | null;
  receivedAt: number;    // epoch ms
}

const STORAGE_KEY = "brilliant.downloadsTray.v1";
const MAX_ITEMS = 20;

function loadFromStorage(): TrayItem[] {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((x): x is TrayItem =>
      typeof x?.filename === "string" && typeof x?.savedPath === "string",
    );
  } catch {
    return [];
  }
}

function saveToStorage(items: TrayItem[]) {
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(items));
  } catch {
    // Quota / private mode — non-fatal.
  }
}

function basename(path: string): string {
  const sep = path.includes("\\") ? "\\" : "/";
  const i = path.lastIndexOf(sep);
  return i >= 0 ? path.slice(i + 1) : path;
}

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

function fileIcon(filename: string, mime: string | null): string {
  const ext = filename.split(".").pop()?.toLowerCase() ?? "";
  if (mime?.startsWith("image/")) return "fa-file-image";
  if (mime === "application/pdf" || ext === "pdf") return "fa-file-pdf";
  if (mime === "application/zip" || ext === "zip") return "fa-file-archive";
  if (["doc", "docx"].includes(ext)) return "fa-file-word";
  if (["xls", "xlsx", "csv"].includes(ext)) return "fa-file-excel";
  if (["ppt", "pptx"].includes(ext)) return "fa-file-powerpoint";
  if (["txt", "md"].includes(ext)) return "fa-file-lines";
  return "fa-file";
}

export default function DownloadsTray() {
  const [items, setItems] = useState<TrayItem[]>(() => loadFromStorage());
  const [collapsed, setCollapsed] = useState(true);

  useEffect(() => {
    saveToStorage(items);
  }, [items]);

  useEffect(() => {
    const unlistenP = listen<DownloadResult>("download://saved", (e) => {
      const p = e.payload;
      if (!p?.saved_path) return;
      const next: TrayItem = {
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        filename: p.filename || basename(p.saved_path),
        savedPath: p.saved_path,
        mime: p.mime ?? null,
        receivedAt: Date.now(),
      };
      setItems((prev) => [next, ...prev].slice(0, MAX_ITEMS));
      // Auto-expand when something new arrives so the user sees it land.
      setCollapsed(false);
    });
    return () => {
      unlistenP.then((fn) => fn()).catch(() => {});
    };
  }, []);

  async function reveal(item: TrayItem) {
    try {
      await api.revealInFolder(item.savedPath);
    } catch (e) {
      alert(`Could not reveal: ${String((e as { message?: string })?.message ?? e)}`);
    }
  }

  async function copyPath(item: TrayItem) {
    try {
      await navigator.clipboard.writeText(item.savedPath);
    } catch {
      // Older webviews: fall back to a hidden textarea.
      const ta = document.createElement("textarea");
      ta.value = item.savedPath;
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy"); } catch { /* ignore */ }
      ta.remove();
    }
  }

  function clear() { setItems([]); }
  function dismiss(id: string) {
    setItems((prev) => prev.filter((x) => x.id !== id));
  }

  function onDragStart(e: React.DragEvent<HTMLDivElement>, item: TrayItem) {
    // Best-effort native drag-out. On macOS WKWebView and recent WebView2 the
    // OS file manager may accept a `text/uri-list` with a file:// URI;
    // otherwise this is a no-op and the "Show in Finder" / "Copy path"
    // buttons cover the use case.
    const url = `file://${encodeURI(item.savedPath)}`;
    try {
      e.dataTransfer.effectAllowed = "copy";
      e.dataTransfer.setData("DownloadURL", `${item.mime ?? "application/octet-stream"}:${item.filename}:${url}`);
      e.dataTransfer.setData("text/uri-list", url);
      e.dataTransfer.setData("text/plain", item.savedPath);
    } catch {
      // Some webviews disallow setData on certain types — non-fatal.
    }
  }

  if (items.length === 0) return null;

  return (
    <div
      className="downloads-tray"
      style={{
        position: "fixed",
        right: 16,
        bottom: 16,
        zIndex: 1000,
        width: collapsed ? 220 : 340,
        maxHeight: collapsed ? 44 : "60vh",
        background: "white",
        boxShadow: "0 6px 24px rgba(0,0,0,0.18)",
        borderRadius: 10,
        border: "1px solid rgba(0,0,0,0.08)",
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        className="is-flex is-align-items-center is-justify-content-space-between"
        style={{
          padding: "8px 12px",
          background: "#f5f5f5",
          borderBottom: collapsed ? "none" : "1px solid #eaeaea",
          cursor: "pointer",
          userSelect: "none",
        }}
        onClick={() => setCollapsed((c) => !c)}
        title={collapsed ? "Expand downloads" : "Collapse downloads"}
      >
        <span className="is-size-7 has-text-weight-semibold">
          <i className="fas fa-download mr-2"></i>Downloads ({items.length})
        </span>
        <span className="icon is-small has-text-grey">
          <i className={`fas ${collapsed ? "fa-chevron-up" : "fa-chevron-down"}`}></i>
        </span>
      </div>

      {!collapsed && (
        <>
          <div style={{ overflowY: "auto", flex: "1 1 auto" }}>
            {items.map((it) => (
              <div
                key={it.id}
                draggable
                onDragStart={(e) => onDragStart(e, it)}
                className="download-row"
                style={{
                  padding: "8px 12px",
                  borderBottom: "1px solid #f0f0f0",
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  cursor: "grab",
                }}
                title="Drag to Finder/Explorer (best effort), or use the buttons below"
              >
                <span className="icon has-text-grey">
                  <i className={`fas ${fileIcon(it.filename, it.mime)}`}></i>
                </span>
                <div style={{ flex: "1 1 auto", minWidth: 0 }}>
                  <div
                    className="is-size-7 has-text-weight-semibold"
                    style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                    title={it.filename}
                  >
                    {it.filename}
                  </div>
                  <div className="is-size-7 has-text-grey">{formatTime(it.receivedAt)}</div>
                </div>
                <button
                  className="button is-small is-light"
                  title="Show in Finder/Explorer"
                  onClick={() => reveal(it)}
                >
                  <span className="icon is-small"><i className="fas fa-folder-open"></i></span>
                </button>
                <button
                  className="button is-small is-light"
                  title="Copy path"
                  onClick={() => copyPath(it)}
                >
                  <span className="icon is-small"><i className="fas fa-copy"></i></span>
                </button>
                <button
                  className="button is-small is-light"
                  title="Dismiss"
                  onClick={() => dismiss(it.id)}
                >
                  <span className="icon is-small"><i className="fas fa-times"></i></span>
                </button>
              </div>
            ))}
          </div>
          <div
            style={{
              padding: "6px 12px",
              borderTop: "1px solid #eaeaea",
              background: "#fafafa",
              display: "flex",
              justifyContent: "flex-end",
            }}
          >
            <button className="button is-small is-text" onClick={clear}>Clear</button>
          </div>
        </>
      )}
    </div>
  );
}
