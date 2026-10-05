// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import CourseRoomPanel from "./CourseRoomPanel";
import { api } from "../api";
import { findCourseRoom, type SyllabusRoom } from "../lib/syllabusRoom";

vi.mock("../lib/syllabusRoom", () => ({ findCourseRoom: vi.fn() }));
vi.mock("../api", () => ({ api: { updateCourseRoom: vi.fn() } }));

describe("course overview room number", () => {
  let container: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    vi.resetAllMocks();
    vi.mocked(api.updateCourseRoom).mockResolvedValue();
  });
  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  it("shows the room and the syllabus it came from", async () => {
    vi.mocked(findCourseRoom).mockResolvedValue({ room: "Science Hall 214", source: "Biology syllabus.pdf" });
    await act(async () => root.render(<CourseRoomPanel courseId="biology" />));
    expect(container.textContent).toContain("Room number");
    expect(container.textContent).toContain("Science Hall 214");
    expect(container.textContent).toContain("From Biology syllabus.pdf");
  });

  it("shows a clear empty state when the syllabus has no room", async () => {
    vi.mocked(findCourseRoom).mockResolvedValue(null);
    await act(async () => root.render(<CourseRoomPanel courseId="biology" />));
    expect(container.textContent).toContain("No room number found in the syllabus.");
  });

  it("distinguishes an unreadable syllabus from one with no room", async () => {
    vi.mocked(findCourseRoom).mockRejectedValue(new Error("Offline"));
    await act(async () => root.render(<CourseRoomPanel courseId="biology" />));
    expect(container.textContent).toContain("Could not read the syllabus.");
  });

  it("does not show a late result from the previous course", async () => {
    let finishOld!: (room: SyllabusRoom) => void;
    vi.mocked(findCourseRoom).mockImplementation((id) => id === "biology"
      ? new Promise((resolve) => { finishOld = resolve; })
      : Promise.resolve({ room: "Payson Hall 305", source: "History syllabus" }));
    await act(async () => root.render(<CourseRoomPanel courseId="biology" />));
    expect(container.textContent).toContain("Checking syllabus…");
    await act(async () => root.render(<CourseRoomPanel courseId="history" />));
    await act(async () => finishOld({ room: "Science Hall 214", source: "Biology syllabus" }));
    expect(container.textContent).toContain("Payson Hall 305");
    expect(container.textContent).not.toContain("Science Hall 214");
  });

  it("reads the updated syllabus after a course sync", async () => {
    vi.mocked(findCourseRoom).mockResolvedValueOnce({ room: "214", source: "Syllabus" })
      .mockResolvedValueOnce({ room: "305", source: "Syllabus" });
    await act(async () => root.render(<CourseRoomPanel courseId="biology" revision={0} />));
    await act(async () => root.render(<CourseRoomPanel courseId="biology" revision={1} />));
    expect(container.textContent).toContain("305");
    expect(findCourseRoom).toHaveBeenCalledTimes(2);
  });

  function click(label: string) {
    [...container.querySelectorAll("button")].find((button) => button.textContent === label)!.click();
  }
  function changeRoom(value: string) {
    const input = container.querySelector("input")!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }
  function submitRoom() {
    container.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  }

  it("saves an edited room and keeps it when the syllabus changes", async () => {
    const onRoomUpdated = vi.fn();
    vi.mocked(findCourseRoom).mockResolvedValue({ room: "214", source: "Syllabus" });
    await act(async () => root.render(<CourseRoomPanel courseId="biology" onRoomUpdated={onRoomUpdated} />));
    await act(async () => click("Edit room"));
    expect(container.querySelector("input")!.value).toBe("214");
    await act(async () => changeRoom("  Science Hall 305  "));
    await act(async () => submitRoom());
    expect(api.updateCourseRoom).toHaveBeenCalledWith("biology", "Science Hall 305");
    expect(onRoomUpdated).toHaveBeenCalledWith("Science Hall 305");
    vi.mocked(findCourseRoom).mockResolvedValue({ room: "999", source: "Syllabus" });
    await act(async () => root.render(<CourseRoomPanel courseId="biology" revision={1} />));
    expect(container.textContent).toContain("Science Hall 305");
    expect(container.textContent).toContain("Edited by you");
    expect(container.textContent).not.toContain("999");
  });

  it("loads a saved override and can restore the syllabus room", async () => {
    vi.mocked(findCourseRoom).mockResolvedValue({ room: "214", source: "Syllabus" });
    await act(async () => root.render(<CourseRoomPanel courseId="biology" customRoom="305" />));
    expect(container.textContent).toContain("305");
    await act(async () => click("Use syllabus"));
    expect(api.updateCourseRoom).toHaveBeenCalledWith("biology", null);
    expect(container.textContent).toContain("214");
    expect(container.textContent).toContain("From Syllabus");
  });

  it("allows setting a room even when the syllabus is unavailable", async () => {
    vi.mocked(findCourseRoom).mockRejectedValue(new Error("Offline"));
    await act(async () => root.render(<CourseRoomPanel courseId="biology" />));
    await act(async () => click("Set room"));
    await act(async () => changeRoom("305"));
    await act(async () => submitRoom());
    expect(container.textContent).toContain("305");
    expect(container.textContent).toContain("Edited by you");
  });

  it("keeps the previous room and draft if saving fails", async () => {
    vi.mocked(findCourseRoom).mockResolvedValue({ room: "214", source: "Syllabus" });
    vi.mocked(api.updateCourseRoom).mockRejectedValue(new Error("Save failed"));
    await act(async () => root.render(<CourseRoomPanel courseId="biology" />));
    await act(async () => click("Edit room"));
    await act(async () => changeRoom("305"));
    await act(async () => submitRoom());
    expect(container.querySelector("input")!.value).toBe("305");
    expect(container.textContent).toContain("Could not save");
    await act(async () => click("Cancel"));
    expect(container.textContent).toContain("214");
    expect(container.textContent).not.toContain("305");
  });

  it("does not carry a room override into another course", async () => {
    vi.mocked(findCourseRoom).mockResolvedValue({ room: "214", source: "Syllabus" });
    await act(async () => root.render(<CourseRoomPanel courseId="biology" customRoom="305" />));
    await act(async () => root.render(<CourseRoomPanel courseId="history" customRoom={null} />));
    expect(container.textContent).not.toContain("305");
    expect(container.textContent).toContain("214");
  });
});
