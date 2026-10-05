// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import CourseSchedulePanel from "./CourseSchedulePanel";
import { api } from "../api";

vi.mock("../api", () => ({ api: { updateCourseSchedule: vi.fn() } }));
const info = { days: "Monday, Wednesday", daysSource: "Syllabus", time: "9:30–10:45 AM", timeSource: "Syllabus" };

describe("editable course meeting schedule", () => {
  let container: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
    vi.resetAllMocks(); vi.mocked(api.updateCourseSchedule).mockResolvedValue();
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });
  const click = (label: string) => [...container.querySelectorAll("button")].find((b) => b.textContent === label)!.click();
  function change(id: string, value: string) {
    const input = container.querySelector<HTMLInputElement>(`#${id}`)!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }
  const submit = () => container.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  it("shows days and time from the syllabus", async () => {
    await act(async () => root.render(<CourseSchedulePanel courseId="a" info={info} />));
    expect(container.textContent).toContain("Monday, Wednesday");
    expect(container.textContent).toContain("9:30–10:45 AM");
    expect(container.textContent).toContain("From Syllabus");
  });
  it("edits one field while keeping the other automatic across refreshes", async () => {
    const updated = vi.fn();
    await act(async () => root.render(<CourseSchedulePanel courseId="a" info={info} onScheduleUpdated={updated} />));
    await act(async () => click("Edit schedule"));
    await act(async () => change("time-a", "  10:00–11:15 AM  "));
    await act(async () => submit());
    expect(api.updateCourseSchedule).toHaveBeenCalledWith("a", null, "10:00–11:15 AM");
    expect(updated).toHaveBeenCalledWith(null, "10:00–11:15 AM");
    await act(async () => root.render(<CourseSchedulePanel courseId="a" info={{ ...info, days: "Thursday", time: "2 PM" }} onScheduleUpdated={updated} />));
    expect(container.textContent).toContain("Thursday");
    expect(container.textContent).toContain("10:00–11:15 AM");
    expect(container.textContent).not.toContain("2 PM");
  });
  it("restores both fields from the syllabus", async () => {
    await act(async () => root.render(<CourseSchedulePanel courseId="a" info={info} customDays="Friday" customTime="2 PM" />));
    await act(async () => click("Use syllabus schedule"));
    expect(api.updateCourseSchedule).toHaveBeenCalledWith("a", null, null);
    expect(container.textContent).toContain(info.days);
    expect(container.textContent).toContain(info.time);
  });
  it("preserves the draft on save failure", async () => {
    vi.mocked(api.updateCourseSchedule).mockRejectedValue(new Error("offline"));
    await act(async () => root.render(<CourseSchedulePanel courseId="a" info={info} />));
    await act(async () => click("Edit schedule"));
    await act(async () => change("days-a", "Friday"));
    await act(async () => submit());
    expect(container.querySelector<HTMLInputElement>("#days-a")!.value).toBe("Friday");
    expect(container.textContent).toContain("Could not save");
    await act(async () => click("Cancel"));
    expect(container.textContent).toContain(info.days);
  });
  it("ignores a late save after switching courses", async () => {
    let finish!: () => void;
    vi.mocked(api.updateCourseSchedule).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    const updated = vi.fn();
    await act(async () => root.render(<CourseSchedulePanel courseId="a" info={info} onScheduleUpdated={updated} />));
    await act(async () => click("Edit schedule"));
    await act(async () => change("days-a", "Friday"));
    await act(async () => submit());
    await act(async () => root.render(<CourseSchedulePanel courseId="b" info={null} onScheduleUpdated={updated} />));
    await act(async () => finish());
    expect(updated).not.toHaveBeenCalled();
    expect(container.textContent).not.toContain("Friday");
    expect(container.textContent).toContain("Checking syllabus");
  });
  it("can save the newly selected course after switching", async () => {
    await act(async () => root.render(<CourseSchedulePanel courseId="a" info={info} customDays="Monday" />));
    await act(async () => root.render(<CourseSchedulePanel courseId="b" info={info} customDays={null} />));
    await act(async () => click("Edit schedule"));
    await act(async () => change("days-b", "Friday"));
    await act(async () => submit());
    expect(api.updateCourseSchedule).toHaveBeenCalledWith("b", "Friday", null);
    expect(container.querySelector("form")).toBeNull();
    expect(container.textContent).toContain("Friday");
  });
  it("allows manual values when there is no readable syllabus", async () => {
    await act(async () => root.render(<CourseSchedulePanel courseId="a" info={{ error: "Could not read the syllabus." }} />));
    await act(async () => click("Set schedule"));
    await act(async () => change("days-a", "Friday"));
    await act(async () => change("time-a", "2 PM"));
    await act(async () => submit());
    expect(container.textContent).toContain("Friday");
    expect(container.textContent).toContain("2 PM");
  });
});
