import { describe, expect, it } from "vitest";
import { compareAssignmentsByDueDate } from "./assignments";
import type { Assignment } from "../types";

function mk(name: string, due: string | null): Assignment {
  return { name, due_date: due } as Assignment;
}

function sortedNames(items: Assignment[]): string[] {
  return [...items].sort(compareAssignmentsByDueDate).map((a) => a.name);
}

describe("compareAssignmentsByDueDate", () => {
  it("orders across the three stored formats chronologically", () => {
    // Days far enough apart that timezone interpretation can't flip them.
    const items = [
      mk("ruby-import", "2026-05-10 09:00:00"),
      mk("brightspace", "2026-05-04T03:59:00.000Z"),
      mk("local-edit", "2026-05-07T23:59:00"),
    ];
    expect(sortedNames(items)).toEqual(["brightspace", "local-edit", "ruby-import"]);
  });

  it("orders same-day naive formats by time of day", () => {
    // Space-separated sorts before 'T' lexicographically ('' < 'T') — the
    // comparator must not fall for that.
    const items = [
      mk("evening", "2026-05-04T20:00:00"),
      mk("morning", "2026-05-04 09:00:00"),
    ];
    expect(sortedNames(items)).toEqual(["morning", "evening"]);
  });

  it("puts rows without a due date last, sorted by name", () => {
    const items = [
      mk("b-none", null),
      mk("dated", "2026-05-04T12:00:00"),
      mk("a-none", null),
      mk("garbage", "not a date"),
    ];
    expect(sortedNames(items)).toEqual(["dated", "a-none", "b-none", "garbage"]);
  });

  it("breaks equal due dates by name", () => {
    const items = [
      mk("zeta", "2026-05-04T12:00:00"),
      mk("alpha", "2026-05-04T12:00:00"),
    ];
    expect(sortedNames(items)).toEqual(["alpha", "zeta"]);
  });
});
