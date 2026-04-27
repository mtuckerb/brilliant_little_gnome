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
} from "./types";

// All backend access goes through these wrappers. Each one corresponds to a
// `#[tauri::command]` in src-tauri/src/commands/*.rs.

export const api = {
  // Auth & status
  authStatus: () => invoke<AuthStatus>("auth_status"),
  setupCookies: (host: string, cookieString: string) =>
    invoke<AuthStatus>("setup_cookies", { host, cookieString }),
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
  updateCourseUnits: (id: string, units: number) =>
    invoke<void>("update_course_units", { id, units }),
  updateCourseTargetGrade: (id: string, target: number) =>
    invoke<void>("update_course_target_grade", { id, target }),
  dropCourse: (id: string, status: string) =>
    invoke<void>("drop_course", { id, status }),
  refreshCourse: (id: string) => invoke<void>("refresh_course", { id }),

  // Grades
  gradesSummary: (courseId: string, showHidden = false) =>
    invoke<{ grades: GradeRow[]; grade_stats: GradeStats }>("grades_summary", {
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
  syncAll: (full: boolean) => invoke<void>("sync_all", { full }),
  syncCourse: (id: string, full: boolean) =>
    invoke<void>("sync_course", { id, full }),

  // REST API toggle
  restApiStart: () => invoke<{ port: number; key: string }>("rest_api_start"),
  restApiStop: () => invoke<void>("rest_api_stop"),
  restApiStatus: () =>
    invoke<{ running: boolean; port: number | null }>("rest_api_status"),
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
