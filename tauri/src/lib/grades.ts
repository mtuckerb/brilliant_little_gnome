import type { GradeRow } from "../types";

export function isActuallyGraded(grade: GradeRow): boolean {
  return grade.is_graded && !grade.is_expected;
}

export function canSpeculateGrade(grade: GradeRow): boolean {
  return !grade.manually_marked_ungraded || grade.is_extra_credit;
}
