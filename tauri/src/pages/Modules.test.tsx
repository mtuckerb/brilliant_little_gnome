// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, onAppEvent, type AppEvent } from "../api";
import type { ContentItem, Course } from "../types";
import Modules from "./Modules";

vi.mock("../api", () => ({ onAppEvent: vi.fn(async () => () => {}), api: {
  getCourse: vi.fn(), listModules: vi.fn(), listCourseItems: vi.fn(), openUrl: vi.fn(async () => {}),
} }));
vi.mock("../components/HeaderBand", () => ({ default: () => null }));
vi.mock("../components/BrightspaceLink", () => ({ useBrightspaceHost: () => "school.test" }));
vi.mock("../components/ToastProvider", () => ({ useToast: () => ({ show: vi.fn() }) }));
vi.mock("../components/SyntheticTaskModal", () => ({ default: (props: { open: boolean; initialDescription: string }) =>
  props.open ? <div data-task>{props.initialDescription}</div> : null,
}));

describe("course material destinations", () => {
  let container: HTMLDivElement;
  let root: Root;
  const reading: ContentItem = {
    id: 1, module_id: "module", brightspace_id: "123", title: "Chapter 4",
    item_type: "File", url: "/content/reading.pdf", is_hidden: false, sort_order: 0,
  };
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    vi.clearAllMocks();
    localStorage.clear();
    vi.mocked(api.getCourse).mockResolvedValue({ org_unit_id: "course", name: "Course" } as Course);
    vi.mocked(api.listModules).mockResolvedValue([{
      id: 1, course_id: "course", brightspace_id: "module", title: "Week 1",
      description: null, sort_order: 0, parent_id: null,
    }]);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  async function render(item: ContentItem) {
    vi.mocked(api.listCourseItems).mockResolvedValue([item]);
    await act(async () => root.render(
      <MemoryRouter initialEntries={["/course/course/content"]}>
        <Routes>
          <Route path="/course/:id/content" element={<Modules />} />
          <Route path="/viewer" element={<div>Brilliant viewer</div>} />
        </Routes>
      </MemoryRouter>,
    ));
  }

  it("opens synced files in Zotero and uses Zotero in generated task notes", async () => {
    const url = "zotero://open-pdf/library/items/ABCD1234";
    await render({ ...reading, zotero_url: url });
    await act(async () => container.querySelector<HTMLButtonElement>('button[title="Open in Zotero"]')!.click());
    expect(api.openUrl).toHaveBeenCalledWith(url);
    expect(container.textContent).not.toContain("Brilliant viewer");
    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="Create a task from Chapter 4"]')!.click());
    expect(container.querySelector("[data-task]")?.textContent).toContain(url);
    expect(container.querySelector("[data-task]")?.textContent).not.toContain("/content/reading.pdf");
  });

  it("keeps unsynced files in the Brilliant viewer", async () => {
    await render(reading);
    await act(async () => container.querySelector<HTMLButtonElement>('button[title="Open in Brilliant"]')!.click());
    expect(container.textContent).toContain("Brilliant viewer");
    expect(api.openUrl).not.toHaveBeenCalled();
  });

  it("automatically updates an open course when previous Zotero documents are discovered", async () => {
    let onEvent: ((event: AppEvent) => void) | undefined;
    const stop = vi.fn();
    vi.mocked(onAppEvent).mockImplementationOnce(async (callback) => { onEvent = callback; return stop; });
    await render(reading);
    expect(container.querySelector('button[title="Open in Brilliant"]')).not.toBeNull();
    const url = "zotero://open-pdf/library/items/ABCD1234";
    vi.mocked(api.listCourseItems).mockResolvedValue([{ ...reading, zotero_url: url }]);
    await act(async () => onEvent!({ kind: "zotero_links_updated" }));
    await act(async () => container.querySelector<HTMLButtonElement>('button[title="Open in Zotero"]')!.click());
    expect(api.openUrl).toHaveBeenCalledWith(url);
    expect(api.listCourseItems).toHaveBeenCalledTimes(2);
    act(() => root.render(<div />));
    expect(stop).toHaveBeenCalledOnce();
  });

  it("keeps external links on their existing Brightspace destination", async () => {
    await render({ ...reading, item_type: "Link", url: "/d2l/quicklink?type=quiz" });
    await act(async () => container.querySelector<HTMLButtonElement>('button[title="Open in Brightspace"]')!.click());
    expect(api.openUrl).toHaveBeenCalledWith("https://school.test/d2l/quicklink?type=quiz");
  });
});
