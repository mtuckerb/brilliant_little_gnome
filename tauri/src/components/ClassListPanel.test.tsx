// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import CourseDetail from "../pages/CourseDetail";
import ClassListPanel from "./ClassListPanel";
import type { CourseRoster } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("./BrightspaceLink", () => ({ useBrightspaceHost: () => "school.test" }));
vi.mock("./ToastProvider", () => ({ useToast: () => ({ show: vi.fn() }) }));
vi.mock("./HeaderBand", () => ({ default: () => null }));
vi.mock("./SyllabusPanel", () => ({ default: () => null }));
vi.mock("./SyntheticTasksPanel", () => ({ default: () => null }));
vi.mock("../lib/download", () => ({ triggerDownload: vi.fn() }));

const roster: CourseRoster = {
  synced_at: "2026-09-30 12:00:00",
  people: [{
    brightspace_user_id: "student-1", display_name: "Ada Example",
    first_name: "Ada", last_name: "Example", email: "ada@school.test",
    role_name: "Student", pronouns: null, is_current_user: false,
  }],
};

describe("course class lists", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command) => {
      switch (command) {
        case "get_course": return { org_unit_id: "course-1", name: "Biology" };
        case "get_prefs": return { cache_content: false };
        case "course_cache_status": return { count: 0, bytes: 0 };
        case "get_course_roster": return roster;
        case "refresh_course_roster": throw new Error("Offline");
        case "download_course_roster": return { filename: "Class-List.csv", saved_path: "/tmp/Class-List.csv" };
        default: throw new Error(`Unexpected command: ${command}`);
      }
    });
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  function button(label: string): HTMLButtonElement {
    return [...container.querySelectorAll("button")].find((element) => element.textContent === label)!;
  }

  it("shows the saved students on the course Overview and downloads that course's list", async () => {
    await act(async () => root.render(
      <MemoryRouter initialEntries={["/course/course-1"]}>
        <Routes><Route path="/course/:id" element={<CourseDetail />} /></Routes>
      </MemoryRouter>,
    ));
    expect(container.textContent).toContain("Class list");
    expect(container.textContent).toContain("Ada Example");
    expect(container.textContent).toContain("1 classmate");
    expect(invoke).toHaveBeenCalledWith("get_course_roster", { courseId: "course-1" });
    await act(async () => button("Download CSV").click());
    expect(invoke).toHaveBeenCalledWith("download_course_roster", { courseId: "course-1" });
  });

  it("keeps the saved students visible when refresh fails", async () => {
    await act(async () => root.render(<ClassListPanel courseId="course-1" />));
    await act(async () => button("Refresh").click());
    expect(invoke).toHaveBeenCalledWith("refresh_course_roster", { courseId: "course-1" });
    expect(container.textContent).toContain("Your last saved copy is still here.");
    expect(container.textContent).toContain("Ada Example");
    expect(button("Refresh").disabled).toBe(false);
  });

  it("does not display another course's students when the new course cannot load", async () => {
    await act(async () => root.render(<ClassListPanel courseId="course-1" />));
    vi.mocked(invoke).mockRejectedValueOnce(new Error("No access"));
    await act(async () => root.render(<ClassListPanel courseId="course-2" />));
    expect(invoke).toHaveBeenCalledWith("get_course_roster", { courseId: "course-2" });
    expect(container.textContent).not.toContain("Ada Example");
    expect(container.textContent).toContain("No access");
    expect(button("Download CSV").disabled).toBe(true);
  });
});
