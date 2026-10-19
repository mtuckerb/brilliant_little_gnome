// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import Grades from "./Grades";
import type { GradeRow, GradeStats } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("../components/HeaderBand", () => ({ default: () => null }));

function grade(overrides: Partial<GradeRow> = {}): GradeRow {
  return {
    id: 12, course_id: "course-1", brightspace_id: "quiz-1", name: "Quiz",
    displayed_grade: null, numerator: null, denominator: 20, weight: null, due_date: null,
    is_extra_credit: false, hidden: false, manually_marked_ungraded: false,
    expected_score: null, comments: null, perc: null, is_graded: false,
    is_expected: false, rel_weight: 1, submitted: null, submitted_at: null, ...overrides,
  };
}
const stats: GradeStats = {
  score: null, confidence: 0, total_points_earned: 0, total_points_possible: 0,
  all_possible_points: 20, remaining_points: 20, target_grade: 93,
  required_avg: 93, is_impossible: false,
};

describe("provisional grades", () => {
  let container: HTMLDivElement;
  let root: Root;
  let row: GradeRow;
  let summaryStats: GradeStats;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    row = grade();
    summaryStats = { ...stats };
    vi.mocked(invoke).mockReset();
    vi.mocked(listen).mockClear();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "grades_summary") return { rows: [row], stats: summaryStats };
      if (command === "set_expected_score") {
        // Mirror the command's argument contract so the old gradeId payload fails.
        const { id, expected } = args as { id: number; expected: number | null };
        if (id !== row.id) throw new Error("Missing grade id");
        row = { ...row, expected_score: expected, is_expected: expected !== null, perc: expected };
        summaryStats = { ...stats, score: expected, total_points_earned: (expected ?? 0) / 100 * 20,
          total_points_possible: expected === null ? 0 : 20, remaining_points: expected === null ? 20 : 0 };
        return;
      }
      if (command.startsWith("toggle_grade_")) return;
      throw new Error(`Unexpected command ${command}`);
    });
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  async function render() {
    await act(async () => root.render(
      <MemoryRouter initialEntries={["/course/course-1/grades"]}>
        <Routes><Route path="/course/:id/grades" element={<Grades />} /></Routes>
      </MemoryRouter>,
    ));
  }
  async function click(selector: string) {
    await act(async () => container.querySelector<HTMLButtonElement>(selector)!.click());
  }
  async function enter(value: string) {
    const input = container.querySelector<HTMLInputElement>("input[type=number]")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }
  async function submit() {
    await act(async () => container.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
  }

  it("saves a provisional percentage, displays estimated points and updates the total", async () => {
    await render();
    await click('[aria-label="Add provisional grade for Quiz"]');
    await enter("90");
    await submit();
    expect(invoke).toHaveBeenCalledWith("set_expected_score", { id: 12, expected: 90 });
    expect(container.textContent).toContain("~90%");
    expect(container.textContent).toContain("~18 / 20");
    expect(container.textContent).toContain("Estimated Grade");
    expect(container.querySelector("tfoot")!.textContent).toContain("18 / 20 pts");
  });

  it("edits a provisional grade and clears it with an empty value", async () => {
    row = grade({ expected_score: 90, is_expected: true, perc: 90 });
    await render();
    await click('[aria-label="Edit provisional grade for Quiz"]');
    expect(container.querySelector("input")!.value).toBe("90");
    await enter("80"); await submit();
    expect(container.textContent).toContain("~16 / 20");
    await click('[aria-label="Edit provisional grade for Quiz"]');
    await enter(""); await submit();
    expect(invoke).toHaveBeenLastCalledWith("grades_summary", { courseId: "course-1", showHidden: false });
    expect(invoke).toHaveBeenCalledWith("set_expected_score", { id: 12, expected: null });
    expect(container.querySelector('[aria-label="Add provisional grade for Quiz"]')).not.toBeNull();
    expect(container.textContent).not.toContain("Estimated Grade");
  });

  it("keeps the input on save failure and allows retry", async () => {
    await render();
    await click('[aria-label="Add provisional grade for Quiz"]');
    await enter("85");
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Disk full"));
    await submit();
    expect(container.querySelector('[role="alert"]')!.textContent).toContain("could not be saved");
    expect(container.querySelector("input")!.value).toBe("85");
    await submit();
    expect(container.textContent).toContain("~85%");
  });

  it("cancels without saving on Escape or Cancel", async () => {
    await render();
    await click('[aria-label="Add provisional grade for Quiz"]');
    await enter("90");
    await act(async () => container.querySelector("input")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(container.querySelector("input")).toBeNull();
    await click('[aria-label="Add provisional grade for Quiz"]');
    await click('button[type="button"].button.is-small:not(.is-ghost)');
    expect(container.querySelector("input")).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("set_expected_score", expect.anything());
  });

  it("replaces the provisional display when an official zero arrives during sync", async () => {
    row = grade({ expected_score: 90, is_expected: true, perc: 90 });
    await render();
    await click('[aria-label="Edit provisional grade for Quiz"]');
    row = grade({ numerator: 0, expected_score: 90, is_graded: true, perc: 0 });
    summaryStats = { ...stats, score: 0, total_points_possible: 20, remaining_points: 0 };
    const callback = vi.mocked(listen).mock.calls.find(([event]) => event === "app-event")![1] as EventCallback<unknown>;
    await act(async () => callback({ event: "app-event", id: 1, payload: {
      kind: "sync_status_changed", status: { status: "idle" },
    } }));
    expect(container.querySelector("input")).toBeNull();
    expect(container.textContent).toContain("0 / 20");
    expect(container.textContent).not.toContain("~90%");
    expect(container.querySelector('[aria-label="Add provisional grade for Quiz"]')).toBeNull();
    expect(container.querySelector('[aria-label="Edit provisional grade for Quiz"]')).toBeNull();
  });

  it("uses the backend id argument for grade controls", async () => {
    await render();
    for (const [title, command] of [["Hide/unhide", "toggle_grade_hidden"], ["Toggle ungraded", "toggle_grade_ungraded"], ["Toggle extra credit", "toggle_grade_extra_credit"]]) {
      await click(`button[title="${title}"]`);
      expect(invoke).toHaveBeenCalledWith(command, { id: 12 });
    }
  });
});
