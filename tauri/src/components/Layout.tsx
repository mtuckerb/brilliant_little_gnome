import type { ReactNode } from "react";
import { Link, useLocation } from "react-router-dom";
import type { AuthStatus, SyncStatus } from "../types";

interface Props {
  auth: AuthStatus;
  sync: SyncStatus | null;
  children: ReactNode;
}

export default function Layout({ auth, sync, children }: Props) {
  const loc = useLocation();
  const tabActive = (path: string) => loc.pathname.startsWith(path) ? "has-text-primary" : "";

  return (
    <>
      <nav className="navbar is-white">
        <div className="navbar-brand">
          <Link className="navbar-item is-size-4 has-text-weight-bold has-text-primary" to="/dashboard">
            Brilliant
          </Link>
        </div>
        <div className="navbar-end">
          <div className="navbar-item">
            <Link to="/calendar" className={`button is-white ${tabActive("/calendar")}`}>
              <span className="icon"><i className="fas fa-calendar-days"></i></span>
              <span>Calendar</span>
            </Link>
          </div>
          <div className="navbar-item">
            <Link to="/archive" className={`button is-white ${tabActive("/archive")}`}>
              <span className="icon"><i className="fas fa-box-archive"></i></span>
              <span>Archive</span>
            </Link>
          </div>
          <div className="navbar-item">
            <Link to="/notifications" className={`button is-white ${tabActive("/notifications")}`}>
              <span className="icon"><i className="fas fa-bell"></i></span>
              <span>Notifications</span>
            </Link>
          </div>
          <div className="navbar-item">
            <span className="icon mr-3" title={auth.degraded ? "Session Expired" : "Authenticated"}>
              <i className={`fas fa-circle ${auth.degraded ? "has-text-danger" : "has-text-success"}`} style={{ fontSize: "0.75rem" }}></i>
            </span>
            <Link to="/settings" className="button is-primary is-light" style={{ border: "2px solid #739AC3", fontWeight: "bold" }}>
              <span className="icon"><i className="fas fa-cog"></i></span>
              <span>Settings</span>
            </Link>
          </div>
        </div>
      </nav>

      {sync && sync.status === "syncing" && (
        <div style={{ background: "white", borderBottom: "2px solid #739AC3", boxShadow: "0 4px 6px -1px rgba(0,0,0,0.05)" }}>
          <div className="container px-5 py-2">
            <div className="is-flex is-justify-content-space-between is-align-items-center">
              <div className="is-flex is-align-items-center" style={{ minWidth: 200 }}>
                <span className="icon is-small has-text-primary mr-3"><i className="fas fa-sync fa-spin"></i></span>
                <span className="is-size-7 has-text-weight-bold has-text-grey-darker">{sync.current_task || "Syncing..."}</span>
              </div>
              <div style={{ flexGrow: 1, margin: "0 20px" }}>
                <progress className="progress is-small is-primary mb-0" value={sync.progress} max={100} style={{ height: 6 }}></progress>
              </div>
              <div style={{ minWidth: 50 }} className="has-text-right">
                <span className="is-size-7 has-text-weight-bold has-text-primary">{sync.progress}%</span>
              </div>
            </div>
          </div>
        </div>
      )}

      <div className="main-content container">{children}</div>
    </>
  );
}
