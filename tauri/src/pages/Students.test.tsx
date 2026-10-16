// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import Students from "./Students";
import type { Student } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("../components/ToastProvider", () => ({ useToast: () => ({ show: vi.fn() }) }));

const png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aWQAAAABJRU5ErkJggg==";
const student: Student = {
  brightspace_user_id: "ada", display_name: "Ada Example", first_name: "Ada", last_name: "Example",
  email: "ada@school.test", pronouns: "she/her", avatar: png, avatar_overridden: true,
  courses: [{ course_id: "old", name: "Biology", semester: "Fall 2025", available: true },
    { course_id: "new", name: "Chemistry", semester: "Fall 2026", available: true }],
};

describe("Students directory", () => {
  let container: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_students") return [structuredClone(student)];
      if (command === "import_students") return { imported_courses: 2, failed_courses: [] };
      if (command === "update_student_avatar") return;
      throw new Error(`Unexpected command ${command}`);
    });
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
  async function render() { await act(async () => root.render(<MemoryRouter><Students /></MemoryRouter>)); }

  it("shows one saved student with current and historical classes and searches semesters", async () => {
    await render();
    expect(container.querySelectorAll("tbody tr")).toHaveLength(1);
    expect(container.textContent).toContain("Biology");
    expect(container.textContent).toContain("Chemistry");
    expect(container.querySelector("img")?.src).toBe(png);
    expect(invoke).toHaveBeenCalledWith("import_students", { force: false });
    const search = container.querySelector<HTMLInputElement>("input[type=search]")!;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    await act(async () => { setter.call(search, "2025"); search.dispatchEvent(new Event("input", { bubbles: true })); });
    expect(container.querySelectorAll("tbody tr")).toHaveLength(1);
    await act(async () => { setter.call(search, "no match"); search.dispatchEvent(new Event("input", { bubbles: true })); });
    expect(container.textContent).toContain("No students match");
  });

  it("keeps saved students and photos when historical import fails", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_students") return [student];
      throw new Error("Offline");
    });
    await render();
    expect(container.textContent).toContain("Ada Example");
    expect(container.textContent).toContain("Saved students are still available");
    expect(container.querySelector("img")?.src).toBe(png);
  });

  it("persists removal as an explicit override", async () => {
    await render();
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="Remove photo for Ada Example"]')!.click());
    expect(invoke).toHaveBeenCalledWith("update_student_avatar", { studentId: "ada", avatar: null });
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toContain("Add photo");
  });

  it("uploads a replacement photo and retains the saved photo on write failure", async () => {
    vi.stubGlobal("FileReader", class {
      result = png;
      onload: (() => void) | null = null;
      readAsDataURL() { this.onload?.(); }
    });
    await render();
    const input = container.querySelector<HTMLInputElement>('input[type="file"]')!;
    Object.defineProperty(input, "files", { value: [new File(["image"], "new.png", { type: "image/png" })] });
    await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
    expect(invoke).toHaveBeenCalledWith("update_student_avatar", { studentId: "ada", avatar: png });
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Disk full"));
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="Remove photo for Ada Example"]')!.click());
    expect(container.querySelector("img")?.src).toBe(png);
    expect(container.textContent).toContain("Photo could not be saved");
  });

  it("rejects unsupported uploads before changing the saved photo", async () => {
    await render();
    const input = container.querySelector<HTMLInputElement>('input[type="file"]')!;
    Object.defineProperty(input, "files", { value: [new File(["svg"], "bad.svg", { type: "image/svg+xml" })] });
    await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
    expect(container.textContent).toContain("smaller than 2 MB");
    expect(invoke).not.toHaveBeenCalledWith("update_student_avatar", expect.anything());
    expect(container.querySelector("img")?.src).toBe(png);
  });
});
