// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { useIsMobile } from "../hooks/useIsMobile";
import Dashboard from "./Dashboard";
import type { Course, DashboardGpa } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("../hooks/useIsMobile", () => ({ useIsMobile: vi.fn(() => false) }));
vi.mock("../components/DueSoon", () => ({ default: () => <div data-testid="due-soon">Due soon</div> }));

const course: Course = {
  org_unit_id: "course-1", name: "Biology", code: "BIO 101", semester: "2026 Fall",
  custom_name: null, custom_code: null, custom_room: null, custom_semester: null,
  custom_meeting_days: null, custom_meeting_time: null, syllabus_meeting_info: null,
  is_pinned: true, custom_color: null, banner_url: null, units: 3, target_grade: 93,
  status: "active", sort_order: 0, end_of_week_day: null, last_accessed_at: null,
};
const initial: DashboardGpa = {
  locked_in_gpa: 3.5, projected_gpa: 3.75, confidence: 0.625,
  locked_in_units: 60, projected_units: 72, provisional_course_count: 1,
  projected_course_count: 4, excluded_course_count: 0,
};

describe("dashboard overall GPA", () => {
  let container: HTMLDivElement;
  let root: Root;
  let summary: DashboardGpa;
  let courses: Course[];
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
    summary = { ...initial }; courses = [course];
    vi.mocked(useIsMobile).mockReturnValue(false);
    vi.mocked(listen).mockClear(); vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_courses") return courses;
      if (command === "dashboard_gpa") return summary;
      throw new Error(`Unexpected command ${command}`);
    });
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });
  async function render() { await act(async () => root.render(<MemoryRouter><Dashboard /></MemoryRouter>)); }
  function text(id: string) { return container.querySelector(`[data-testid="${id}"]`)!.textContent; }
  async function emit(event: string, payload: unknown = null) {
    const calls = vi.mocked(listen).mock.calls.filter(([name]) => name === event);
    await act(async () => {
      for (const [, callback] of calls) (callback as EventCallback<unknown>)({ event, id: 1, payload });
    });
  }

  it("shows credit-weighted GPA and confidence above the desktop course list and due dates", async () => {
    await render();
    expect(text("locked-in-gpa")).toBe("3.50");
    expect(text("projected-gpa")).toBe("3.75");
    expect(text("gpa-confidence")).toBe("63%");
    expect(container.textContent).toContain("60 completed credits");
    expect(container.textContent).toContain("Provisional grades");
    expect(container.textContent).toContain("course targets for remaining work");
    const gpa = container.querySelector("section")!;
    expect(gpa.compareDocumentPosition(container.querySelector('[data-testid="due-soon"]')!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(container.textContent).toContain("Biology");
    expect(invoke).toHaveBeenCalledWith("dashboard_gpa");
  });

  it("also shows the summary at the top of the mobile dashboard", async () => {
    vi.mocked(useIsMobile).mockReturnValue(true);
    await render();
    expect(text("projected-gpa")).toBe("3.75");
    expect(container.querySelector("section")!.compareDocumentPosition(container.querySelector(".pencil-statsRow")!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("shows historic GPA even before any courses are synced", async () => {
    courses = [];
    await render();
    expect(text("locked-in-gpa")).toBe("3.50");
    expect(container.textContent).toContain("No courses yet");
  });

  it("shows unavailable values for missing records, and real zero grades as zero", async () => {
    summary = { ...initial, locked_in_gpa: null, projected_gpa: null, confidence: 0, locked_in_units: 0,
      projected_units: 0, provisional_course_count: 0, projected_course_count: 0, excluded_course_count: 1 };
    await render();
    expect(text("locked-in-gpa")).toBe("—");
    expect(text("projected-gpa")).toBe("—");
    expect(text("gpa-confidence")).toBe("—");
    expect(container.querySelector('a[href="/settings"]')).not.toBeNull();
    expect(container.textContent).toContain("1 course without grade data excluded");
    summary = { ...initial, locked_in_gpa: 0, projected_gpa: 0, confidence: 1, provisional_course_count: 0 };
    await emit("grades:updated");
    expect(text("locked-in-gpa")).toBe("0.00");
    expect(text("projected-gpa")).toBe("0.00");
    expect(text("gpa-confidence")).toBe("100%");
  });

  it("refreshes after grade sync, peer changes and historic GPA edits", async () => {
    await render();
    summary = { ...initial, projected_gpa: 3.8, confidence: 0.9, provisional_course_count: 0 };
    await emit("app-event", { kind: "sync_status_changed", status: { status: "idle" } });
    expect(text("projected-gpa")).toBe("3.80");
    expect(text("gpa-confidence")).toBe("90%");
    expect(container.textContent).not.toContain("Provisional grades");
    summary = { ...summary, locked_in_gpa: 3.6 };
    await emit("prefs:updated");
    expect(text("locked-in-gpa")).toBe("3.60");
    summary = { ...summary, projected_gpa: 3.7 };
    await emit("course:updated");
    expect(text("projected-gpa")).toBe("3.70");
  });

  it("keeps courses and the last good GPA visible on refresh failure and supports retry", async () => {
    await render();
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Offline"));
    await emit("grades:updated");
    expect(container.querySelector('[role="alert"]')!.textContent).toContain("last loaded summary");
    expect(text("projected-gpa")).toBe("3.75");
    expect(container.textContent).toContain("Biology");
    summary = { ...summary, projected_gpa: 3.9 };
    await act(async () => [...container.querySelectorAll("button")].find((button) => button.textContent === "Retry")!.click());
    expect(text("projected-gpa")).toBe("3.90");
    expect(container.querySelector('[role="alert"]')).toBeNull();
  });

  it("handles an initial GPA error independently of the course dashboard", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_courses") return courses;
      throw new Error("Database unavailable");
    });
    await render();
    expect(container.textContent).toContain("GPA could not be loaded");
    expect(container.textContent).toContain("Biology");
    expect(container.textContent).not.toContain("NaN");
  });
});
