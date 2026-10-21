import { describe, expect, it } from "vitest";
import { dueSoonCourseIdentifier } from "./dueSoon";

describe("dueSoonCourseIdentifier", () => {
  const baseCourse = {
    org_unit_id: "447423",
    code: null,
    custom_code: null,
  };

  it("prefers the user-visible custom course code", () => {
    expect(dueSoonCourseIdentifier({ ...baseCourse, code: "SWO 399", custom_code: "Field" })).toBe("Field");
  });

  it("uses the synced course code when no custom code is set", () => {
    expect(dueSoonCourseIdentifier({ ...baseCourse, code: "SWO 399" })).toBe("SWO 399");
  });

  it("falls back to assignment.course_id, then course.org_unit_id", () => {
    expect(dueSoonCourseIdentifier(baseCourse, { course_id: "447423" })).toBe("447423");
    expect(dueSoonCourseIdentifier(baseCourse)).toBe("447423");
  });
});
