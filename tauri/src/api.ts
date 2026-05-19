import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Course,
  GradeRow,
  GradeStats,
  Assignment,
  Notification,
  UserPreferences,
  AuthStatus,
  SyncStatus,
  ContentModule,
  ContentItem,
  DiscussionForum,
  DiscussionTopic,
  DiscussionPost,
  AssignmentDetailPayload,
  P2pStatus,
  PairingQr,
  StorageStats,
} from "./types";

// All backend access goes through these wrappers. Each one corresponds to a
// `#[tauri::command]` in src-tauri/src/commands/*.rs.

export const api = {
  // Auth & status
  authStatus: () => invoke<AuthStatus>("auth_status"),
  setupCookies: (host: string, cookieString: string) =>
    invoke<AuthStatus>("setup_cookies", { host, cookieString }),
  openLoginWindow: (host: string) =>
    invoke<void>("open_login_window", { host }),
  clearAuth: () => invoke<void>("clear_auth"),

  // Preferences
  getPrefs: () => invoke<UserPreferences>("get_prefs"),
  updatePrefs: (patch: Partial<UserPreferences>) =>
    invoke<UserPreferences>("update_prefs", { patch }),

  // Courses
  listCourses: () => invoke<Course[]>("list_courses"),
  getCourse: (id: string) => invoke<Course>("get_course", { id }),
  reorderCourses: (orderedIds: string[]) =>
    invoke<void>("reorder_courses", { orderedIds }),
  updateCourseColor: (id: string, color: string) =>
    invoke<void>("update_course_color", { id, color }),
  updateCourseName: (id: string, name: string) =>
    invoke<Course>("update_course_name", { id, name }),
  updateCourseUnits: (id: string, units: number | null) =>
    invoke<void>("update_course_units", { id, units }),
  updateCourseTargetGrade: (id: string, target: number | null) =>
    invoke<void>("update_course_target_grade", { id, target }),
  updateCourseEndOfWeek: (id: string, day: number) =>
    invoke<void>("update_course_end_of_week", { id, day }),
  dropCourse: (id: string, status: string) =>
    invoke<void>("drop_course", { id, status }),
  refreshCourse: (id: string) => invoke<void>("refresh_course", { id }),

  // Overview / syllabus
  getCourseOverview: (id: string) =>
    invoke<{
      description_html: string | null;
      has_attachment: boolean;
      attachment_name: string | null;
      attachment_url: string | null;
    }>("get_course_overview", { courseId: id }),
  fetchCourseOverviewAttachment: (id: string) =>
    invoke<{ bytes_base64: string; mime: string | null; filename: string }>(
      "fetch_course_overview_attachment",
      { courseId: id },
    ),

  // Grades
  gradesSummary: (courseId: string, showHidden = false) =>
    invoke<{ rows: GradeRow[]; stats: GradeStats }>("grades_summary", {
      courseId,
      showHidden,
    }),
  toggleGradeHidden: (gradeId: number) =>
    invoke<void>("toggle_grade_hidden", { gradeId }),
  toggleGradeUngraded: (gradeId: number) =>
    invoke<void>("toggle_grade_ungraded", { gradeId }),
  toggleGradeExtraCredit: (gradeId: number) =>
    invoke<void>("toggle_grade_extra_credit", { gradeId }),
  setExpectedScore: (gradeId: number, expected: number | null) =>
    invoke<void>("set_expected_score", { gradeId, expected }),

  // Assignments
  listAssignments: (courseId: string, completed?: boolean) =>
    invoke<Assignment[]>("list_assignments", { courseId, completed }),
  toggleAssignmentComplete: (id: number) =>
    invoke<void>("toggle_assignment_complete", { id }),
  toggleAssignmentOptional: (id: number) =>
    invoke<void>("toggle_assignment_optional", { id }),
  updateAssignmentDueDate: (id: number, dueDate: string | null) =>
    invoke<void>("update_assignment_due_date", { id, dueDate }),
  createSyntheticAssignment: (
    courseId: string,
    name: string,
    dueDate: string | null,
    description: string | null,
  ) =>
    invoke<Assignment>("create_synthetic_assignment", {
      courseId,
      name,
      dueDate,
      description,
    }),
  deleteAssignment: (id: number) =>
    invoke<void>("delete_assignment", { id }),
  getAssignmentDetail: (courseId: string, assignmentId: string) =>
    invoke<AssignmentDetailPayload>("get_assignment_detail", {
      courseId,
      assignmentId,
    }),
  previewAttachment: (url: string, filename: string) =>
    invoke<{ bytes_base64: string; mime: string | null; filename: string }>(
      "preview_attachment",
      { url, filename },
    ),

  // Downloads (single file or zipped archive returned as base64)
  downloadTopicFile: (courseId: string, topicId: string) =>
    invoke<{ bytes_base64: string; mime: string | null; filename: string }>(
      "download_topic_file",
      { courseId, topicId },
    ),
  downloadModuleArchive: (courseId: string, moduleId: string) =>
    invoke<{ bytes_base64: string; mime: string | null; filename: string }>(
      "download_module_archive",
      { courseId, moduleId },
    ),
  downloadCourseArchive: (courseId: string) =>
    invoke<{ bytes_base64: string; mime: string | null; filename: string }>(
      "download_course_archive",
      { courseId },
    ),

  // Notifications
  listNotifications: (params: {
    unreadOnly?: boolean;
    courseId?: string | null;
    limit?: number;
  }) => invoke<Notification[]>("list_notifications", params),
  markNotificationRead: (id: number) =>
    invoke<void>("mark_notification_read", { id }),
  markAllNotificationsRead: (courseId?: string | null) =>
    invoke<void>("mark_all_notifications_read", { courseId }),

  // Sync
  syncStatus: () => invoke<SyncStatus>("sync_status"),
  syncAll: (force: boolean) => invoke<void>("sync_all", { force }),
  syncCourse: (id: string) =>
    invoke<void>("sync_course", { courseId: id }),

  // Content
  listModules: (courseId: string) =>
    invoke<ContentModule[]>("list_modules", { courseId }),
  listItems: (moduleId: string) =>
    invoke<ContentItem[]>("list_items", { moduleId }),

  // Discussions
  listForums: (courseId: string) =>
    invoke<DiscussionForum[]>("list_forums", { courseId }),
  listTopics: (courseId: string) =>
    invoke<DiscussionTopic[]>("list_topics", { courseId }),
  listTopicPosts: (courseId: string, topicId: string) =>
    invoke<DiscussionPost[]>("list_topic_posts", { courseId, topicId }),

  // REST API toggle
  restApiStart: () => invoke<{ port: number; key: string }>("rest_api_start"),
  restApiStop: () => invoke<void>("rest_api_stop"),
  restApiStatus: () =>
    invoke<{ running: boolean; port: number | null }>("rest_api_status"),

  // P2P device-to-device sync (T-014). Only callable when the
  // backend is built with the `p2p` feature; otherwise the IPC
  // command is absent and `invoke` will reject. The Settings panel
  // gracefully treats that rejection as "p2p not available".
  p2pStatus: () => invoke<P2pStatus>("p2p_status"),
  p2pEnable: () => invoke<P2pStatus>("p2p_enable"),
  p2pDisable: () => invoke<void>("p2p_disable"),
  p2pPairingQr: () => invoke<PairingQr>("p2p_pairing_qr"),
  p2pConsumePairing: (encoded: string) =>
    invoke<P2pStatus>("p2p_consume_pairing", { args: { encoded } }),
  p2pRotate: () => invoke<P2pStatus>("p2p_rotate"),
  p2pStorageStats: () => invoke<StorageStats>("p2p_storage_stats"),
  // Mobile log capture: iOS doesn't reliably route our Rust tracing
  // output to the device syslog, so we keep our own in-memory ring and
  // hand it out via this command. Surfaced as a "Show log" disclosure in
  // Settings → Sync for on-device pairing debugging.
  p2pDebugLog: () => invoke<string[]>("p2p_debug_log"),
};

// Tauri-event subscriptions (replacing the Ruby SSE stream).
export type AppEvent =
  | { kind: "course_overview_updated"; course_id: string }
  | { kind: "assignments_updated"; course_id: string }
  | { kind: "grades_updated"; course_id: string }
  | { kind: "notification_received"; course_id: string | null; type: string }
  | { kind: "notifications_synced"; count: number }
  | { kind: "discussion_topic_updated"; course_id: string; topic_id: string }
  | { kind: "authentication_failure"; code: number; host: string }
  | { kind: "sync_status_changed"; status: SyncStatus };

export async function onAppEvent(
  cb: (e: AppEvent) => void,
): Promise<UnlistenFn> {
  return await listen<AppEvent>("app-event", (e) => cb(e.payload));
}
