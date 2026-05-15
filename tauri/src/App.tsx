import { useEffect, useState } from "react";
import { Routes, Route, Navigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { api, onAppEvent } from "./api";
import type { AuthStatus, SyncStatus } from "./types";
import Layout from "./components/Layout";
import Dashboard from "./pages/Dashboard";
import CourseDetail from "./pages/CourseDetail";
import Grades from "./pages/Grades";
import Assignments from "./pages/Assignments";
import AssignmentDetail from "./pages/AssignmentDetail";
import Notifications from "./pages/Notifications";
import Settings from "./pages/Settings";
import Setup from "./pages/Setup";
import Calendar from "./pages/Calendar";
import Modules from "./pages/Modules";
import ModuleDetail from "./pages/ModuleDetail";
import Discussions from "./pages/Discussions";
import DiscussionTopic from "./pages/DiscussionTopic";
import Archive from "./pages/Archive";
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

    // `auth-captured` fires from both the desktop login flow and the
    // P2P joiner-side BootstrapCredentials adoption (mobile pairing).
    // In both cases the auth state has just transitioned to good, but
    // we have no Brightspace data yet — kick a sync immediately so the
    // dashboard populates within seconds rather than waiting for the
    // periodic loop.
    const unlistenAuthCaptured = listen<string>("auth-captured", async () => {
      try {
        const a = await api.authStatus();
        setAuth(a);
      } catch {
        // ignore — periodic poll will recover
      }
      api.syncAll(false).catch(() => {});
    });

    const interval = setInterval(() => {
      api.syncStatus().then(setSync).catch(() => {});
    }, 4000);

    return () => {
      clearInterval(interval);
      unlistenP.then((fn) => fn()).catch(() => {});
      unlistenAuthCaptured.then((fn) => fn()).catch(() => {});
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
    <Layout auth={auth} sync={sync} onAuthChange={setAuth}>
      <Routes>
        <Route path="/" element={<Navigate to="/dashboard" replace />} />
        <Route path="/dashboard" element={<Dashboard />} />
        <Route path="/course/:id" element={<CourseDetail />} />
        <Route path="/course/:id/grades" element={<Grades />} />
        <Route path="/course/:id/assignments" element={<Assignments />} />
        <Route path="/course/:id/assignments/:aid" element={<AssignmentDetail />} />
        <Route path="/course/:id/content" element={<Modules />} />
        <Route path="/course/:id/content/:moduleId" element={<ModuleDetail />} />
        <Route path="/course/:id/discussions" element={<Discussions />} />
        <Route path="/course/:id/discussions/:topicId" element={<DiscussionTopic />} />
        <Route path="/notifications" element={<Notifications />} />
        <Route path="/calendar" element={<Calendar />} />
        <Route path="/archive" element={<Archive />} />
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
