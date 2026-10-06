import type { Assignment, Course } from "../types";

export type DueSoonCourse = Pick<Course, "org_unit_id" | "code" | "custom_code">;

export function dueSoonCourseIdentifier(
  course: DueSoonCourse,
  assignment?: Pick<Assignment, "course_id">,
): string {
  return course.custom_code || course.code || assignment?.course_id || course.org_unit_id;
}
