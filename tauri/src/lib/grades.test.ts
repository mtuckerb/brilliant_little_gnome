import { describe, expect, it } from "vitest";
import { canSpeculateGrade, isActuallyGraded } from "./grades";
import type { GradeRow } from "../types";

function grade(overrides: Partial<GradeRow> = {}): GradeRow {
  return {
    id: 1,
    course_id: "course-1",
    brightspace_id: "grade-1",
    name: "Quiz",
    displayed_grade: null,
    numerator: null,
    denominator: 10,
    weight: null,
    due_date: null,
    is_extra_credit: false,
    hidden: false,
    manually_marked_ungraded: false,
    expected_score: null,
    comments: null,
    perc: null,
    is_graded: false,
    is_expected: false,
    rel_weight: 1,
    submitted: null,
    submitted_at: null,
    ...overrides,
  };
}

describe("grade row actions", () => {
  it("allows speculation for ordinary ungraded marks", () => {
    expect(canSpeculateGrade(grade())).toBe(true);
  });

  it("keeps manually ungraded non-extra-credit marks read-only", () => {
    expect(canSpeculateGrade(grade({ manually_marked_ungraded: true }))).toBe(false);
  });

  it("allows speculation for marks that are both manually ungraded and extra credit", () => {
    expect(canSpeculateGrade(grade({ manually_marked_ungraded: true, is_extra_credit: true }))).toBe(true);
  });

  it("does not treat expected scores as actual grades", () => {
    expect(isActuallyGraded(grade({ is_graded: true, is_expected: true }))).toBe(false);
    expect(isActuallyGraded(grade({ is_graded: true, is_expected: false }))).toBe(true);
  });
});
