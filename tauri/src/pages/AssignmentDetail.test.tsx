// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, onAppEvent, type AppEvent } from "../api";
import type { Assignment, ContentItem, Course } from "../types";
import AssignmentDetail from "./AssignmentDetail";

vi.mock("../api", () => ({ onAppEvent: vi.fn(), api: {
  getCourse: vi.fn(), listAssignments: vi.fn(), getAssignmentDetail: vi.fn(),
  listCourseItems: vi.fn(), openUrl: vi.fn(async () => {}),
} }));
vi.mock("../components/HeaderBand", () => ({ default: () => null }));
vi.mock("../components/BrightspaceLink", () => ({ default: () => null, useBrightspaceHost: () => "courses.maine.edu" }));
vi.mock("../components/EditAssignmentModal", () => ({ default: ({ assignment }: { assignment: Assignment | null }) =>
  assignment ? <textarea aria-label="Task notes" value={assignment.description ?? ""} readOnly /> : null,
}));
const mobile = vi.hoisted(() => ({ value: false }));
vi.mock("../hooks/useIsMobile", () => ({ useIsMobile: () => mobile.value }));

describe("existing task source destinations", () => {
  let container: HTMLDivElement;
  let root: Root;
  let onEvent: (event: AppEvent) => void;
  const original = "https://courses.maine.edu/content/enforced/479604-2710.UMS06-S.81454.1/Kanel Ch. 1.pdf";
  const destination = "zotero://open-pdf/library/items/8EEJQ897";
  const notes = `From module **Required Readings**\n\n[Kanel, Chapter 1](<${original}>)`;
  const item: ContentItem = {
    id: 1, module_id: "module", brightspace_id: "12314227", title: "Kanel, Chapter 1",
    item_type: "File", url: original.replace("https://courses.maine.edu", ""),
    is_hidden: false, sort_order: 0, zotero_url: destination,
  };
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
    vi.resetAllMocks(); mobile.value = false;
    vi.mocked(onAppEvent).mockImplementation(async (callback) => { onEvent = callback; return () => {}; });
    vi.mocked(api.getCourse).mockResolvedValue({ org_unit_id: "479604", name: "Methods of SW Practice II" } as Course);
    vi.mocked(api.listAssignments).mockResolvedValue([{
      id: 2147545, course_id: "479604", brightspace_id: "syn_reading", name: item.title,
      synthetic: true, manually_edited: true, description: notes, assignment_type: "synthetic",
    } as Assignment]);
    vi.mocked(api.getAssignmentDetail).mockResolvedValue({ submissions: [], instruction_attachments: [] } as unknown as Awaited<ReturnType<typeof api.getAssignmentDetail>>);
    vi.mocked(api.listCourseItems).mockResolvedValue([item]);
    vi.mocked(api.openUrl).mockResolvedValue();
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });
  async function render() {
    await act(async () => root.render(<MemoryRouter initialEntries={["/course/479604/assignments/2147545"]}>
      <Routes><Route path="/course/:id/assignments/:aid" element={<AssignmentDetail />} /></Routes>
    </MemoryRouter>));
  }
  function sourceLink() { return Array.from(container.querySelectorAll(".content a")).find((a) => a.textContent === "Kanel, Chapter 1")!; }

  it.each([false, true])("opens an old task's synced PDF in Zotero (mobile: %s)", async (isMobile) => {
    mobile.value = isMobile; await render();
    expect(sourceLink().getAttribute("href")).toBe(destination);
    await act(async () => sourceLink().click());
    expect(api.openUrl).toHaveBeenCalledWith(destination);
    expect(api.listCourseItems).toHaveBeenCalledWith("479604");
  });

  it("refreshes an open task after background recovery and keeps original editable notes", async () => {
    vi.mocked(api.listCourseItems).mockResolvedValue([{ ...item, zotero_url: null }]);
    await render();
    expect(sourceLink().getAttribute("href")).toContain("courses.maine.edu");
    vi.mocked(api.listCourseItems).mockResolvedValue([item]);
    await act(async () => onEvent({ kind: "zotero_links_updated" }));
    expect(sourceLink().getAttribute("href")).toBe(destination);
    const edit = Array.from(container.querySelectorAll("button")).find((b) => b.textContent?.includes("Edit"))!;
    await act(async () => edit.click());
    expect(container.querySelector("textarea")?.value).toBe(notes);
  });

  it("preserves the source when Zotero mapping lookup fails", async () => {
    vi.mocked(api.listCourseItems).mockRejectedValue(new Error("unavailable"));
    await render();
    expect(sourceLink().getAttribute("href")).toContain("courses.maine.edu");
    expect(container.textContent).toContain("Instructions");
  });
});
