import { describe, expect, it } from "vitest";
import type { ContentModule } from "../types";
import { dateFromModuleTitle, moduleTaskDue } from "./moduleTasks";

function module(
  brightspace_id: string,
  title: string,
  parent_id: string | null = null,
): ContentModule {
  return {
    id: Number(brightspace_id),
    course_id: "course-1",
    brightspace_id,
    title,
    description: null,
    sort_order: null,
    parent_id,
  };
}

describe("dateFromModuleTitle", () => {
  it("parses the long dated week heading shown in the module tree", () => {
    const date = dateFromModuleTitle("Week 1: September 2, 2026", 2030);
    expect(date && [date.getFullYear(), date.getMonth(), date.getDate()]).toEqual([2026, 8, 2]);
  });

  it("uses the supplied course year for a numeric heading without a year", () => {
    const date = dateFromModuleTitle("Week 3 - 1/19", 2027);
    expect(date && [date.getFullYear(), date.getMonth(), date.getDate()]).toEqual([2027, 0, 19]);
  });

  it("rejects impossible dates", () => {
    expect(dateFromModuleTitle("Week 4: February 30, 2026")).toBeNull();
  });

  it("does not treat chapter fractions as dates", () => {
    expect(dateFromModuleTitle("Chapters 4/5", 2026)).toBeNull();
  });
});

describe("moduleTaskDue", () => {
  it("defaults nested content to the course's last class day in its ancestor week", () => {
    const modules = [
      module("1", "Week 1: September 2, 2026"),
      module("2", "Required Readings", "1"),
    ];

    // September 2, 2026 is Wednesday; Friday is the configured final class day.
    expect(moduleTaskDue("2", modules, 5)).toBe("2026-09-04T23:59");
  });

  it("uses the nearest dated ancestor and safely stops on malformed cycles", () => {
    const modules = [
      module("1", "Week 1: September 2, 2026", "2"),
      module("2", "Required Readings", "1"),
    ];
    expect(moduleTaskDue("2", modules, 4)).toBe("2026-09-03T23:59");
    expect(moduleTaskDue("missing", modules, 4)).toBeUndefined();

    const undatedCycle = [
      module("3", "Required Media", "4"),
      module("4", "PowerPoint", "3"),
    ];
    expect(moduleTaskDue("3", undatedCycle, 4)).toBeUndefined();
  });

  it("ignores an ambiguous numeric child title and inherits the dated week", () => {
    const modules = [
      module("1", "Week 3: September 16, 2026"),
      module("2", "Chapters 4/5", "1"),
    ];
    expect(moduleTaskDue("2", modules, 5)).toBe("2026-09-18T23:59");
  });
});
