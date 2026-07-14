import { useEffect, useState } from "react";
import { Routes, Route, Navigate, useLocation, useNavigate } from "react-router-dom";
import { parseCoursePath, recordCourseVisit } from "./lib/courseLastTab";
import { listen } from "@tauri-apps/api/event";
import { api, onAppEvent } from "./api";
import type { AuthStatus, SyncStatus } from "./types";
import DesktopVision from "./components/DesktopVision";
import MobileLayout from "./components/MobileLayout";
import { useIsMobile } from "./hooks/useIsMobile";
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
import FileViewer from "./pages/FileViewer";
import Discussions from "./pages/Discussions";
import DiscussionTopic from "./pages/DiscussionTopic";
import CourseAnnouncements from "./pages/CourseAnnouncements";
import CourseSearch from "./pages/CourseSearch";
import Archive from "./pages/Archive";
import { ToastProvider, useToast } from "./components/ToastProvider";
import DownloadsTray from "./components/DownloadsTray";
import { useExternalLinkInterceptor } from "./hooks/useExternalLinkInterceptor";

// In-shell re-authentication screen. Reachable while the session is degraded
// (tappable auth dot / Settings) so the phone isn't a dead end when it has no
// native sign-in — it surfaces the cookie-paste + re-pair flow. Reuses Setup.
function Reauth({ onComplete, host }: { onComplete: (a: AuthStatus) => void; host: string | null }) {
  const navigate = useNavigate();
  return (
    <Setup
      reauth
      initialHost={host ?? undefined}
      onComplete={(a) => {
        onComplete(a);
        navigate("/dashboard");
      }}
    />
  );
}

function CourseTabRecorder() {
  const location = useLocation();
  useEffect(() => {
    const parsed = parseCoursePath(location.pathname);
    if (parsed) recordCourseVisit(parsed.courseId, parsed.subPath);
  }, [location.pathname]);
  return null;
}

function AppInner() {
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [sync, setSync] = useState<SyncStatus | null>(null);
  const toast = useToast();
  useExternalLinkInterceptor();
  const isMobile = useIsMobile();

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

    // Desktop OTA: the Rust side checks for updates on launch and emits this.
    const unlistenUpdate = listen<{ version?: string }>("update://available", (e) => {
      toast.show(
        `Update ${e.payload?.version ?? ""} available — Settings → Software update to install.`,
        "is-info",
        9000,
      );
    });

    const interval = setInterval(() => {
      api.syncStatus().then(setSync).catch(() => {});
    }, 4000);

    return () => {
      clearInterval(interval);
      unlistenP.then((fn) => fn()).catch(() => {});
      unlistenAuthCaptured.then((fn) => fn()).catch(() => {});
      unlistenUpdate.then((fn) => fn()).catch(() => {});
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

  const Shell = isMobile ? MobileLayout : DesktopVision;
  return (
    <Shell auth={auth} sync={sync} onAuthChange={setAuth}>
      <CourseTabRecorder />
      <Routes>
        <Route path="/" element={<Navigate to="/dashboard" replace />} />
        <Route path="/dashboard" element={<Dashboard />} />
        <Route path="/course/:id" element={<CourseDetail />} />
        <Route path="/course/:id/grades" element={<Grades />} />
        <Route path="/course/:id/assignments" element={<Assignments />} />
        <Route path="/course/:id/assignments/:aid" element={<AssignmentDetail />} />
        <Route path="/viewer" element={<FileViewer />} />
        <Route path="/course/:id/content" element={<Modules />} />
        <Route path="/course/:id/content/:moduleId" element={<ModuleDetail />} />
        <Route path="/course/:id/discussions" element={<Discussions />} />
        <Route path="/course/:id/discussions/:topicId" element={<DiscussionTopic />} />
        <Route path="/course/:id/announcements" element={<CourseAnnouncements />} />
        <Route path="/course/:id/search" element={<CourseSearch />} />
        <Route path="/notifications" element={<Notifications />} />
        <Route path="/calendar" element={<Calendar />} />
        <Route path="/archive" element={<Archive />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="/reauth" element={<Reauth onComplete={setAuth} host={auth.host} />} />
        <Route path="*" element={<Navigate to="/dashboard" replace />} />
      </Routes>
      <DownloadsTray />
    </Shell>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <AppInner />
    </ToastProvider>
  );
}
