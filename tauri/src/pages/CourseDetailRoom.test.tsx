// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import CourseDetail from "./CourseDetail";
import { api } from "../api";
import { findCourseRoom } from "../lib/syllabusRoom";
import type { Course } from "../types";

vi.mock("../api", () => ({ api: {
  getCourse: vi.fn(), getPrefs: vi.fn(), courseCacheStatus: vi.fn(), updateCourseRoom: vi.fn(),
} }));
vi.mock("../lib/syllabusRoom", () => ({ findCourseRoom: vi.fn() }));
vi.mock("../components/ToastProvider", () => ({ useToast: () => ({ show: vi.fn() }) }));
vi.mock("../components/SyllabusPanel", () => ({ default: () => null }));
vi.mock("../components/ClassListPanel", () => ({ default: () => null }));
vi.mock("../components/SyntheticTasksPanel", () => ({ default: () => null }));
vi.mock("../components/HeaderBand", () => ({
  default: ({ courseId, onCourseUpdated }: { courseId: string; onCourseUpdated: (course: Course) => void }) =>
    <button onClick={() => onCourseUpdated({ org_unit_id: courseId, name: "Updated title", custom_room: null } as Course)}>Edit title</button>,
}));

describe("room overrides on the course overview", () => {
  it("preserves a newly saved room when the header sends an older course snapshot", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.mocked(api.getCourse).mockResolvedValue({ org_unit_id: "biology", name: "Biology", custom_room: null } as Course);
    vi.mocked(api.getPrefs).mockResolvedValue({ cache_content: false } as Awaited<ReturnType<typeof api.getPrefs>>);
    vi.mocked(api.courseCacheStatus).mockResolvedValue({ count: 0, bytes: 0, last_cached_at: null });
    vi.mocked(api.updateCourseRoom).mockResolvedValue();
    vi.mocked(findCourseRoom).mockResolvedValue({ room: "214", source: "Syllabus" });
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const button = (label: string) => [...container.querySelectorAll("button")].find((item) => item.textContent === label)!;
    try {
      await act(async () => root.render(
        <MemoryRouter initialEntries={["/course/biology"]}>
          <Routes><Route path="/course/:id" element={<CourseDetail />} /></Routes>
        </MemoryRouter>,
      ));
      await act(async () => button("Edit room").click());
      const input = container.querySelector<HTMLInputElement>("input[type=text]")!;
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "305");
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await act(async () => container.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
      await act(async () => button("Edit title").click());
      expect(api.updateCourseRoom).toHaveBeenCalledWith("biology", "305");
      expect(container.textContent).toContain("305");
      expect(container.textContent).toContain("Edited by you");
    } finally {
      act(() => root.unmount());
      container.remove();
    }
  });
});
