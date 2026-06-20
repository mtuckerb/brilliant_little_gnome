import { type ReactNode } from "react";
import { Link, useLocation } from "react-router-dom";
import type { AuthStatus, SyncStatus } from "../types";
import { api, onAppEvent } from "../api";
import GnomeSync from "./GnomeSync";
import PullToRefresh from "./PullToRefresh";

// Trigger a sync and resolve only once it actually finishes, so the
// pull-to-refresh spinner reflects real progress. We subscribe to the sync
// status transition first (avoiding a race), then kick a forced sync unless one
// is already running. A timeout backstops the promise so the spinner can never
// hang if the done-event is missed.
async function syncAndWait(): Promise<void> {
  let sawSyncing = false;
  let settled = false;
  let resolveDone: () => void = () => {};
  const done = new Promise<void>((r) => (resolveDone = r));
  const finish = () => {
    if (settled) return;
    settled = true;
    clearTimeout(timer);
    unlistenP.then((fn) => fn()).catch(() => {});
    resolveDone();
  };
  const timer = setTimeout(finish, 45000);
  const unlistenP = onAppEvent((e) => {
    if (e.kind !== "sync_status_changed") return;
    if (e.status.status === "syncing") sawSyncing = true;
    else if (sawSyncing) finish();
  });
  const st = await api.syncStatus().catch(() => null);
  if (st?.status === "syncing") sawSyncing = true;
  else await api.syncAll(true).catch(() => {});
  await done;
}

// iPhone-sized mobile shell. Matches the three Pencil frames
// (`Mobile Vision - Navigation` HDUr0, `Mobile Vision - Assignment` c3DdR,
// `Mobile Vision - Calendar` NouE8): dark `appBar` on top, content fills
// the middle, persistent `bottomNav` with four icons.
//
// All paint colors come from --pencil-* design tokens defined in
// styles/pencil-tokens.css (which mirrors the .pen file's `variables`
// block verbatim). The frame names appear here as className markers so
// devtools maps directly back to the Pencil file.
//
// Phase 1: the existing pages render unchanged inside the content area —
// they're already responsive enough to fit at 390px wide. Per-page mobile
// styling (the card-on-mobile look from the mockup) is a follow-up.

interface Props {
  auth: AuthStatus;
  sync: SyncStatus | null;
  onAuthChange: (auth: AuthStatus) => void;
  children: ReactNode;
}

interface Tab {
  to: string;
  icon: string;
  label: string;
  matchPrefix: string;
}

// Pencil frame: bottomNav (HDUr0 children: navHome/navLearn/navInbox/navProfile).
const TABS: Tab[] = [
  { to: "/dashboard", icon: "fa-book", label: "Courses", matchPrefix: "/dashboard" },
  { to: "/calendar", icon: "fa-calendar-days", label: "Calendar", matchPrefix: "/calendar" },
  { to: "/notifications", icon: "fa-bell", label: "Alerts", matchPrefix: "/notifications" },
  { to: "/settings", icon: "fa-cog", label: "Settings", matchPrefix: "/settings" },
];

export default function MobileLayout({ auth, sync, children }: Props) {
  const location = useLocation();
  const authOk = !auth.degraded;
  const authDot = auth.degraded
    ? "var(--pencil-danger)"
    : "var(--pencil-success)";

  return (
    <div
      className="pencil-mobile-shell"
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100dvh",
        paddingTop: "env(safe-area-inset-top)",
        overflow: "hidden",
      }}
    >
      <header
        className="pencil-appBar"
        style={{
          flex: "0 0 auto",
          padding: "12px 14px",
          display: "flex",
          alignItems: "center",
          gap: 10,
          borderBottom: "1px solid rgba(0,0,0,0.3)",
          position: "relative",
        }}
      >
        <Link
          to="/dashboard"
          style={{
            color: "var(--pencil-text-on-dark)",
            textDecoration: "none",
            display: "flex",
            alignItems: "center",
            gap: 8,
            position: "absolute",
            left: "50%",
            transform: "translateX(-50%)",
          }}
        >
          <GnomeSync size={24} brightspaceSync={sync} brightspaceOk={authOk} />
          <span style={{ fontWeight: 700 }}>Brilliant</span>
        </Link>
        <span style={{ flex: "1 1 auto" }} />
        <Link
          to="/reauth"
          className="icon is-small"
          title={auth.degraded ? "Session expired — tap to re-authenticate" : "Account — tap to re-authenticate"}
          aria-label={auth.degraded ? "Session expired, re-authenticate" : "Account"}
          style={{ color: authDot, textDecoration: "none" }}
        >
          <i className="fas fa-circle" style={{ fontSize: "0.55rem" }}></i>
        </Link>
      </header>

      <PullToRefresh
        onRefresh={syncAndWait}
        style={{ flex: "1 1 auto", background: "var(--pencil-bg-app)" }}
      >
        <div style={{ padding: "12px 12px 16px 12px" }}>{children}</div>
      </PullToRefresh>

      <nav
        className="pencil-bottomNav"
        style={{
          flex: "0 0 auto",
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: "8px 18px",
          paddingBottom: "max(8px, env(safe-area-inset-bottom))",
        }}
      >
        {TABS.map((t) => {
          const active = location.pathname.startsWith(t.matchPrefix);
          return (
            <Link
              key={t.to}
              to={t.to}
              className={`pencil-bottomNav-item${active ? " is-active" : ""}`}
            >
              <i className={`fas ${t.icon}`} style={{ fontSize: 18 }}></i>
              <span>{t.label}</span>
            </Link>
          );
        })}
      </nav>
    </div>
  );
}
