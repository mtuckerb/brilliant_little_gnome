// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import CourseDetail from "./CourseDetail";
import { api, onCourseUpdated } from "../api";
import { findCourseMeetingInfo } from "../lib/syllabusRoom";
import type { Course } from "../types";

vi.mock("../api", () => ({ api: { getCourse: vi.fn(), getPrefs: vi.fn(), courseCacheStatus: vi.fn(), cacheCourseMeetingInfo: vi.fn() }, onCourseUpdated: vi.fn() }));
vi.mock("../lib/syllabusRoom", async (original) => ({ ...await original<typeof import("../lib/syllabusRoom")>(), findCourseMeetingInfo: vi.fn() }));
vi.mock("../components/ToastProvider", () => ({ useToast: () => ({ show: vi.fn() }) }));
vi.mock("../components/SyllabusPanel", () => ({ default: () => null }));
vi.mock("../components/ClassListPanel", () => ({ default: () => null }));
vi.mock("../components/SyntheticTasksPanel", () => ({ default: () => null }));
vi.mock("../components/HeaderBand", () => ({ default: () => null }));

const shared = { room: "Science Hall 214", roomSource: "Syllabus", days: "Monday, Wednesday", daysSource: "Syllabus", time: "9:30–10:45 AM", timeSource: "Syllabus" };
const makeCourse = (patch: Partial<Course> = {}) => ({ org_unit_id: "biology", name: "Biology", custom_room: null, custom_meeting_days: null, custom_meeting_time: null, syllabus_meeting_info: null, ...patch } as Course);

describe("paired course detail updates", () => {
  let container: HTMLDivElement;
  let root: Root;
  let emit: (id: string) => void;
  let stop: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.resetAllMocks();
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
    stop = vi.fn();
    vi.mocked(onCourseUpdated).mockImplementation(async (listener) => { emit = listener; return stop; });
    vi.mocked(api.getPrefs).mockResolvedValue({ cache_content: false } as Awaited<ReturnType<typeof api.getPrefs>>);
    vi.mocked(api.courseCacheStatus).mockResolvedValue({ count: 0, bytes: 0, last_cached_at: null });
    vi.mocked(findCourseMeetingInfo).mockRejectedValue(new Error("No local syllabus"));
  });
  afterEach(async () => { await act(async () => root.unmount()); container.remove(); });
  async function render() {
    await act(async () => root.render(<MemoryRouter initialEntries={["/course/biology"]}><Routes><Route path="/course/:id" element={<CourseDetail />} /></Routes></MemoryRouter>));
  }
  it("refreshes all peer edits live and uses the shared syllabus after a peer reset", async () => {
    vi.mocked(api.getCourse).mockResolvedValue(makeCourse({ syllabus_meeting_info: JSON.stringify(shared) }));
    await render();
    expect(container.textContent).toContain(shared.room);
    expect(container.textContent).toContain(shared.days);
    expect(container.textContent).toContain(shared.time);
    vi.mocked(api.getCourse).mockResolvedValue(makeCourse({ syllabus_meeting_info: JSON.stringify(shared), custom_room: "305", custom_meeting_days: "Friday", custom_meeting_time: "2 PM" }));
    await act(async () => emit("biology"));
    expect(container.textContent).toContain("305");
    expect(container.textContent).toContain("Friday");
    expect(container.textContent).toContain("2 PM");
    vi.mocked(api.getCourse).mockResolvedValue(makeCourse({ syllabus_meeting_info: JSON.stringify(shared) }));
    await act(async () => emit("biology"));
    expect(container.textContent).toContain(shared.room);
    expect(container.textContent).toContain(shared.days);
    expect(container.textContent).toContain(shared.time);
    expect(container.textContent).not.toContain("305");
    expect(findCourseMeetingInfo).toHaveBeenCalledTimes(1);
    expect(api.cacheCourseMeetingInfo).not.toHaveBeenCalled();
  });
  it("ignores other courses and releases its event listener", async () => {
    vi.mocked(api.getCourse).mockResolvedValue(makeCourse());
    await render();
    await act(async () => emit("history"));
    expect(api.getCourse).toHaveBeenCalledTimes(1);
    await act(async () => root.unmount());
    expect(stop).toHaveBeenCalledTimes(1);
    await act(async () => emit("biology"));
    expect(api.getCourse).toHaveBeenCalledTimes(1);
  });
  it("keeps the newest received update when refresh responses finish out of order", async () => {
    vi.mocked(api.getCourse).mockResolvedValue(makeCourse());
    await render();
    let older!: (course: Course) => void;
    vi.mocked(api.getCourse).mockImplementationOnce(() => new Promise((resolve) => { older = resolve; }))
      .mockResolvedValueOnce(makeCourse({ custom_room: "999", custom_meeting_days: "Thursday", custom_meeting_time: "3 PM" }));
    await act(async () => { emit("biology"); emit("biology"); });
    await act(async () => older(makeCourse({ custom_room: "old", custom_meeting_days: "Monday", custom_meeting_time: "9 AM" })));
    expect(container.textContent).toContain("999");
    expect(container.textContent).toContain("Thursday");
    expect(container.textContent).toContain("3 PM");
    expect(container.textContent).not.toContain("old");
  });
});
