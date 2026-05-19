// URL builders for Brightspace D2L deep-links. The host comes from
// user_preferences.brightspace_host; everything else is reconstructed from
// the IDs Brilliant already knows. These mirror the URL patterns the D2L
// UI uses — if Brightspace ever changes them we'll catch it via the icon
// landing on a 404, not silent breakage.

export function courseHomeUrl(host: string, courseId: string): string {
  return `https://${host}/d2l/home/${courseId}`;
}

export function moduleUrl(host: string, courseId: string, moduleId: string): string {
  // The "Content/Home" route accepts an `identifier` query that scrolls /
  // selects the module in the LMS sidebar.
  return `https://${host}/d2l/le/content/${courseId}/Home?identifier=${moduleId}`;
}

export function topicViewUrl(host: string, courseId: string, topicId: string): string {
  return `https://${host}/d2l/le/content/${courseId}/viewContent/${topicId}/View`;
}

export function assignmentSubmitUrl(host: string, courseId: string, folderId: string): string {
  return `https://${host}/d2l/lms/dropbox/user/folder_submit_files.d2l?db=${folderId}&ou=${courseId}`;
}

// Discussion URLs use the legacy `/d2l/lms/discussions/...` routes — the
// `/d2l/le/<api>/...` shape from the REST API path 404s in the web UI.
export function discussionForumUrl(host: string, courseId: string, forumId: string): string {
  return `https://${host}/d2l/lms/discussions/admin/forum_view.d2l?ou=${courseId}&forumId=${forumId}`;
}

export function discussionTopicUrl(host: string, courseId: string, topicId: string): string {
  return `https://${host}/d2l/lms/discussions/messageLists.d2l?ou=${courseId}&topicId=${topicId}`;
}
