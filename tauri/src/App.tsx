import { useEffect, useState } from "react";
import { Routes, Route, Navigate } from "react-router-dom";
import { api, onAppEvent } from "./api";
import type { AuthStatus, SyncStatus } from "./types";
import Layout from "./components/Layout";
import Dashboard from "./pages/Dashboard";
import CourseDetail from "./pages/CourseDetail";
import Grades from "./pages/Grades";
import Assignments from "./pages/Assignments";
import Notifications from "./pages/Notifications";
import Settings from "./pages/Settings";
import Setup from "./pages/Setup";
import { ToastProvider, useToast } from "./components/ToastProvider";

function AppInner() {
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [sync, setSync] = useState<SyncStatus | null>(null);
  const toast = useToast();

  useEffect(() => {
    api.authStatus().then(setAuth).catch(() => setAuth({ authenticated: false, degraded: false, host: null, user_id: null, uid: null }));
    api.syncStatus().then(setSync).catch(() => {});

    const unlistenP = onAppEvent((e) => {
      if (e.kind === "sync_status_changed") setSync(e.status);
      if (e.kind === "notification_received") {
        toast.show(`New notification${e.course_id ? "" : ""}`, "is-success");
      }
      if (e.kind === "authentication_failure") {
        setAuth((a) => (a ? { ...a, degraded: true } : a));
        toast.show("Session expired — re-authenticate", "is-danger");
      }
    });

    const interval = setInterval(() => {
      api.syncStatus().then(setSync).catch(() => {});
    }, 4000);

    return () => {
      clearInterval(interval);
      unlistenP.then((fn) => fn()).catch(() => {});
    };
  }, []);

  if (auth === null) {
    return (
      <div style={{ padding: 60, textAlign: "center" }}>
        <span className="icon is-large has-text-primary">
          <i className="fas fa-circle-notch fa-spin fa-3x"></i>
        </span>
      </div>
    );
  }

  if (!auth.authenticated) {
    return (
      <Routes>
        <Route path="*" element={<Setup onComplete={(a) => setAuth(a)} />} />
      </Routes>
    );
  }

  return (
    <Layout auth={auth} sync={sync}>
      <Routes>
        <Route path="/" element={<Navigate to="/dashboard" replace />} />
        <Route path="/dashboard" element={<Dashboard />} />
        <Route path="/course/:id" element={<CourseDetail />} />
        <Route path="/course/:id/grades" element={<Grades />} />
        <Route path="/course/:id/assignments" element={<Assignments />} />
        <Route path="/notifications" element={<Notifications />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/dashboard" replace />} />
      </Routes>
    </Layout>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <AppInner />
    </ToastProvider>
  );
}
