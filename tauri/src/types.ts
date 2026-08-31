// Types mirror the Rust structs in src-tauri/src/models.rs.
// Keep them in sync.

export interface Course {
  org_unit_id: string;
  name: string;
  custom_name: string | null;
  code: string | null;
  custom_code: string | null;
  semester: string | null;
  custom_semester: string | null;
  is_pinned: boolean;
  custom_color: string | null;
  banner_url: string | null;
  units: number | null;
  target_grade: number | null;
  status: string | null;
  sort_order: number | null;
  end_of_week_day: number | null;
  last_accessed_at: string | null;
}

export function displayCourseName(course: Course): string {
  return course.custom_name || course.name;
}

export function displayCourseCode(course: Course): string | null {
  return course.custom_code || course.code;
}

export function displayCourseSemester(course: Course): string | null {
  return course.custom_semester || course.semester;
}

// Strip one-or-more leading course-code tokens (e.g. "SWO-399", "SWO 399",
// "SWO399", "SWO-399/SWO-599") plus the separator that follows. Used when we
// render `${code} ${title}` so a name that already begins with the code
// doesn't produce "SWO-399 - SWO-399 Topics in Social Work".
const LEADING_CODE_RE = /^\s*(?:[A-Z]{2,4}\s*-?\s*\d{3,4})(?:\s*[/,&]\s*[A-Z]{2,4}\s*-?\s*\d{3,4})*\s*[-:–—]?\s*/i;

export function stripLeadingCode(name: string): string {
  const stripped = name.replace(LEADING_CODE_RE, "").trim();
  // Defensive: if the strip ate the whole string, keep the original so we
  // never render an empty title.
  return stripped.length > 0 ? stripped : name;
}

// One-stop helper for rendering "{code} — {clean name}" in lists/rows. Keeps
// the code prefix when known and strips it off the name to avoid the dup.
export function courseLabel(course: Course): string {
  const code = displayCourseCode(course);
  const name = stripLeadingCode(displayCourseName(course));
  return code ? `${code} — ${name}` : name;
}

export interface Grade {
  id: number;
  course_id: string;
  brightspace_id: string | null;
  name: string;
  displayed_grade: string | null;
  numerator: number | null;
  denominator: number | null;
  weight: number | null;
  due_date: string | null;
  is_extra_credit: boolean;
  hidden: boolean;
  manually_marked_ungraded: boolean;
  expected_score: number | null;
  comments: string | null;
}

export interface GradeRow extends Grade {
  perc: number | null;
  is_graded: boolean;
  is_expected: boolean;
  rel_weight: number;
  submitted: boolean | null;
  submitted_at: string | null;
}

export interface GradeStats {
  score: number | null;
  confidence: number;
  total_points_earned: number;
  total_points_possible: number;
  all_possible_points: number;
  remaining_points: number;
  target_grade: number;
  required_avg: number | null;
  is_impossible: boolean;
}

export interface Assignment {
  id: number;
  course_id: string;
  brightspace_id: string;
  name: string;
  due_date: string | null;
  description: string | null;
  is_graded: boolean;
  grade_item_id: string | null;
  assignment_type: string | null;
  completed: boolean;
  completed_at: string | null;
  synthetic: boolean;
  optional: boolean;
  external_url: string | null;
  manually_edited: boolean;
}

export interface Notification {
  id: number;
  external_id: string;
  notification_type: string;
  title: string;
  body: string | null;
  date: string | null;
  course_id: string | null;
  course_name: string | null;
  urgency: number;
  is_personal: boolean;
  is_read: boolean;
  url: string | null;
}

export interface UserPreferences {
  id: number;
  display_name: string | null;
  time_zone: string | null;
  brightspace_host: string | null;
  api_enabled: boolean;
  api_key: string | null;
  api_listen_all: boolean;
  api_port: number;
  jwt_secret: string | null;
  semester_colors: Record<string, string>;
  historic_gpa: number | null;
  historic_units: number | null;
  default_semester: string | null;
  brightspace_uid: string | null;
  brightspace_user_id: string | null;
  last_login_at: string | null;
  calendar_show_empty_days: boolean;
  cache_content: boolean;
  spotify_client_id: string | null;
  spotify_client_secret: string | null;
  zotero_user_id: string | null;
  zotero_api_key: string | null;
  zotero_use_local: boolean;
  zotero_local_base_url: string | null;
  zotero_local_user_id: string | null;
  zotero_basic_auth_user: string | null;
  zotero_basic_auth_pass: string | null;
}

export interface AuthStatus {
  authenticated: boolean;
  degraded: boolean;
  host: string | null;
  user_id: string | null;
  uid: string | null;
}

export interface SyncStatus {
  status: "idle" | "syncing" | "error";
  current_task: string | null;
  progress: number;
  last_sync_at: string | null;
}

export interface ContentModule {
  id: number;
  course_id: string;
  brightspace_id: string;
  title: string;
  description: string | null;
  sort_order: number | null;
  parent_id: string | null;
}

export interface ContentItem {
  id: number;
  module_id: string;
  brightspace_id: string;
  title: string;
  item_type: string | null;
  url: string | null;
  is_hidden: boolean;
  sort_order: number | null;
}

export interface DiscussionForum {
  id: number;
  brightspace_id: string;
  course_id: string;
  name: string;
  description: string | null;
}

export interface DiscussionTopic {
  id: number;
  brightspace_id: string;
  course_id: string;
  forum_id: string;
  name: string;
  description: string | null;
  sort_order: number | null;
  thread_count: number | null;
  post_count: number | null;
  last_post_date: string | null;
}

export interface AssignmentAttachment {
  name: string;
  url: string | null;
  size: number | null;
}

export interface AssignmentSubmissionDetail {
  submitted_at: string | null;
  comment_html: string | null;
  files: AssignmentAttachment[];
}

export interface AssignmentFeedback {
  displayed_score: string | null;
  score_numerator: number | null;
  score_denominator: number | null;
  feedback_html: string | null;
  attachments: AssignmentAttachment[];
}

export interface GradebookEntry {
  displayed_grade: string | null;
  numerator: number | null;
  denominator: number | null;
  comments_html: string | null;
}

export interface AssignmentDetailPayload {
  folder_raw: unknown | null;
  rubrics_raw: unknown | null;
  feedback: AssignmentFeedback | null;
  submissions: AssignmentSubmissionDetail[];
  instructions_html: string | null;
  instruction_attachments: AssignmentAttachment[];
  gradebook: GradebookEntry | null;
  synthetic: boolean;
}

export interface DiscussionPost {
  post_id: string;
  parent_post_id: string | null;
  thread_id: string | null;
  subject: string | null;
  body_html: string | null;
  author_name: string | null;
  author_id: string | null;
  posted_at: string | null;
  is_pinned: boolean;
}

// ---- P2P device-to-device sync (T-014/T-015) -----------------------------

export interface PairedPeer {
  nodeId: string;
  lastSeenAt: string | null;
}

export interface P2pStatus {
  enabled: boolean;
  nodeId: string | null;
  pairedPeers: PairedPeer[];
  lastApplyAt: string | null;
}

export interface PairingPayload {
  v: number;
  node: string;
  addrs: string[];
  relay: string | null;
  secret: string;
  nonce: string;
  exp: number;
}

export interface PairingQr {
  payload: PairingPayload;
  pngB64: string;
  encoded: string;
}

export interface StorageStats {
  snapshotBytes: number;
  walBytes: number;
  walEntries: number;
}
