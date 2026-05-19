import { useState, type ReactNode } from "react";
import { Link, useLocation } from "react-router-dom";
import type { AuthStatus, SyncStatus } from "../types";
import SidebarCourseList from "./SidebarCourseList";
import StatusBar from "./StatusBar";
import GnomeSync from "./GnomeSync";

// Desktop shell: thin title bar, dark left sidebar with semester-grouped
// courses + footer icons + fuzzy search, main content pane on the right,
// status bar at the bottom. Replaces the old Bulma top-nav Layout.

interface Props {
  auth: AuthStatus;
  sync: SyncStatus | null;
  onAuthChange: (auth: AuthStatus) => void;
  children: ReactNode;
}

const SIDEBAR_WIDTH = 240;

export default function DesktopLayout({ auth, sync, onAuthChange, children }: Props) {
  const [search, setSearch] = useState("");
  const location = useLocation();

  const isOnRoute = (prefix: string) => location.pathname.startsWith(prefix);
  const authOk = !auth.degraded;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        // 100dvh fits the actual visible viewport on iOS (100vh includes
        // the status bar / home indicator regions and overflows). Safe-area
        // insets push the title bar below the iOS status bar.
        height: "100dvh",
        paddingTop: "env(safe-area-inset-top)",
        paddingBottom: "env(safe-area-inset-bottom)",
        overflow: "hidden",
        background: "#F4F5F7",
      }}
    >
      <TitleBar />

      <div style={{ flex: "1 1 auto", display: "flex", overflow: "hidden" }}>
        <aside
          style={{
            width: SIDEBAR_WIDTH,
            flex: "0 0 auto",
            background: "#2A3744",
            display: "flex",
            flexDirection: "column",
            borderRight: "1px solid rgba(0,0,0,0.2)",
          }}
        >
          <SidebarBrand sync={sync} authOk={authOk} />
          <SidebarCourseList filter={search} />
          <SidebarFooter
            search={search}
            onSearchChange={setSearch}
            isOnRoute={isOnRoute}
          />
        </aside>

        <main
          style={{
            flex: "1 1 auto",
            overflow: "auto",
            background: "#F4F5F7",
          }}
        >
          <div style={{ padding: "20px 24px" }}>{children}</div>
        </main>
      </div>

      <StatusBar auth={auth} sync={sync} onAuthChange={onAuthChange} />
    </div>
  );
}

function TitleBar() {
  return (
    <div
      style={{
        height: 32,
        flex: "0 0 auto",
        background: "#1F2A33",
        color: "#9DA9B5",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: "0 12px",
        fontSize: 12,
        borderBottom: "1px solid rgba(0,0,0,0.3)",
      }}
    >
      <span style={{ fontWeight: 600, color: "#FFFFFF" }}>Brilliant</span>
    </div>
  );
}

function SidebarBrand({ sync, authOk }: { sync: SyncStatus | null; authOk: boolean }) {
  return (
    <Link
      to="/dashboard"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "16px 14px",
        textDecoration: "none",
        color: "#FFFFFF",
        borderBottom: "1px solid rgba(0,0,0,0.2)",
      }}
    >
      <GnomeSync size={44} brightspaceSync={sync} brightspaceOk={authOk} />
      <div style={{ display: "flex", flexDirection: "column" }}>
        <span style={{ fontWeight: 700, fontSize: 15 }}>Brilliant</span>
        <span style={{ fontSize: 11, color: "#9DA9B5" }}>Dashboard</span>
      </div>
    </Link>
  );
}

interface FooterProps {
  search: string;
  onSearchChange: (v: string) => void;
  isOnRoute: (prefix: string) => boolean;
}

function SidebarFooter({ search, onSearchChange, isOnRoute }: FooterProps) {
  const iconStyle = (active: boolean): React.CSSProperties => ({
    color: active ? "#FFFFFF" : "#9DA9B5",
    fontSize: 14,
  });

  return (
    <div
      style={{
        flex: "0 0 auto",
        borderTop: "1px solid rgba(0,0,0,0.2)",
        background: "#243240",
      }}
    >
      <div style={{ padding: "8px 10px" }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            padding: "6px 10px",
            background: "#3A4856",
            borderRadius: 6,
          }}
        >
          <span className="icon is-small" style={{ color: "#7C8896" }}>
            <i className="fas fa-magnifying-glass"></i>
          </span>
          <input
            value={search}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder="Fuzzy search · ⌘K"
            style={{
              flex: "1 1 auto",
              background: "transparent",
              border: "none",
              outline: "none",
              color: "#FFFFFF",
              fontSize: 12,
              minWidth: 0,
            }}
          />
        </div>
      </div>
      <nav
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: "8px 14px",
          gap: 12,
        }}
      >
        <Link to="/calendar" title="Calendar" aria-label="Calendar">
          <i className="fas fa-calendar-days" style={iconStyle(isOnRoute("/calendar"))}></i>
        </Link>
        <Link to="/notifications" title="Notifications" aria-label="Notifications">
          <i className="fas fa-bell" style={iconStyle(isOnRoute("/notifications"))}></i>
        </Link>
        <Link to="/archive" title="Archive" aria-label="Archive">
          <i className="fas fa-box-archive" style={iconStyle(isOnRoute("/archive"))}></i>
        </Link>
        <Link to="/settings" title="Settings" aria-label="Settings">
          <i className="fas fa-cog" style={iconStyle(isOnRoute("/settings"))}></i>
        </Link>
      </nav>
    </div>
  );
}
