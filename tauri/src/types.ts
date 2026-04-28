// Types mirror the Rust structs in src-tauri/src/models.rs.
// Keep them in sync.

export interface Course {
  org_unit_id: string;
  name: string;
  code: string | null;
  semester: string | null;
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
