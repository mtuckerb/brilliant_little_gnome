import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { AuthStatus, SyncStatus } from "../types";
import type { DownloadResult } from "../lib/download";
import { useReauthenticate } from "../hooks/useReauthenticate";

// Bottom strip of the desktop shell. Conveys at-a-glance state without
// stealing weight from the main pane: sync state on the left, recent-
// downloads count in the middle, auth dot on the right. P2P status is
// folded into the same line when we have it.
//
// Designed to be ignorable when everything's green and informative when
// something needs attention.

interface Props {
  auth: AuthStatus;
  sync: SyncStatus | null;
  onAuthChange: (auth: AuthStatus) => void;
}

export default function StatusBar({ auth, sync, onAuthChange }: Props) {
  const [downloadCount, setDownloadCount] = useState(0);
  const [lastDownload, setLastDownload] = useState<string | null>(null);
  const reauth = useReauthenticate(auth.host, onAuthChange);

  useEffect(() => {
    const unlisten = listen<DownloadResult>("download://saved", (e) => {
      setDownloadCount((n) => n + 1);
      if (e.payload?.filename) setLastDownload(e.payload.filename);
    });
    return () => { unlisten.then((fn) => fn()).catch(() => {}); };
  }, []);

  const syncing = sync?.status === "syncing";
  const syncLabel = syncing
    ? sync?.current_task || "Syncing…"
    : sync?.last_sync_at
      ? `Synced ${new Date(sync.last_sync_at).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}`
      : "Idle";

  const authColor = auth.degraded
    ? "var(--pencil-danger)"
    : "var(--pencil-success)";
  const authTitle = auth.degraded
    ? "Session expired — click to re-authenticate"
    : `Authenticated${auth.host ? ` to ${auth.host}` : ""}${auth.uid ? ` as ${auth.uid}` : ""}`;

  return (
    <div
      className="pencil-StatusBar"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 14,
        padding: "6px 14px",
        borderTop: "1px solid rgba(0,0,0,0.25)",
        flex: "0 0 auto",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span
          className="icon is-small"
          style={{
            color: syncing
              ? "var(--pencil-accent)"
              : "var(--pencil-success)",
          }}
        >
          <i className={`fas ${syncing ? "fa-sync fa-spin" : "fa-check"}`}></i>
        </span>
        <span>{syncLabel}</span>
        {syncing && sync && (
          <span style={{ color: "var(--pencil-text-on-dark-faint)" }}>· {sync.progress}%</span>
        )}
      </div>

      <span style={{ flex: "1 1 auto" }} />

      {downloadCount > 0 && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            padding: "4px 10px",
            background: "var(--pencil-sidebar-row-hover)",
            borderRadius: 12,
          }}
          title={lastDownload ? `Most recent: ${lastDownload}` : undefined}
        >
          <span className="icon is-small" style={{ color: "var(--pencil-accent)" }}>
            <i className="fas fa-download"></i>
          </span>
          <span>{downloadCount} download{downloadCount === 1 ? "" : "s"}</span>
        </div>
      )}

      <button
        type="button"
        title={authTitle}
        aria-label={authTitle}
        onClick={() => auth.degraded && !reauth.busy && reauth.reauthenticate()}
        disabled={!auth.degraded || reauth.busy}
        style={{
          background: "transparent",
          border: "none",
          cursor: auth.degraded ? "pointer" : "default",
          padding: 4,
          color: authColor,
        }}
      >
        <span className="icon is-small"><i className="fas fa-circle" style={{ fontSize: "0.55rem" }}></i></span>
      </button>
    </div>
  );
}
