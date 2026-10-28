// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import type { ContentItem, Course } from "../types";
import ModuleDetail from "./ModuleDetail";

vi.mock("../api", () => ({ onAppEvent: vi.fn(async () => () => {}), api: {
  getCourse: vi.fn(), listModules: vi.fn(), listItems: vi.fn(), zoteroSendTopic: vi.fn(),
  openUrl: vi.fn(async () => {}), downloadTopicFile: vi.fn(),
} }));
vi.mock("../components/HeaderBand", () => ({ default: () => null }));
vi.mock("../components/BrightspaceLink", () => ({ default: () => null, useBrightspaceHost: () => "school.test" }));
vi.mock("../components/ToastProvider", () => ({ useToast: () => ({ show: vi.fn() }) }));
vi.mock("../components/SyntheticTaskModal", () => ({ default: () => null }));

describe("linked module resources", () => {
  let container: HTMLDivElement;
  let root: Root;
  const resource: ContentItem = {
    id: 1, module_id: "module", brightspace_id: "123", title: "Required reading",
    item_type: "Link", url: "https://example.test/reading", is_hidden: false, sort_order: 0,
  };
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    vi.clearAllMocks();
    vi.mocked(api.getCourse).mockResolvedValue({ org_unit_id: "course" } as Course);
    vi.mocked(api.listModules).mockResolvedValue([{
      id: 1, course_id: "course", brightspace_id: "module", title: "Week 1",
      description: null, sort_order: 0, parent_id: null,
    }]);
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  async function render(item = resource) {
    vi.mocked(api.listItems).mockResolvedValue([item]);
    await act(async () => root.render(
      <MemoryRouter initialEntries={["/course/course/content/module"]}>
        <Routes><Route path="/course/:id/content/:moduleId" element={<ModuleDetail />} /></Routes>
      </MemoryRouter>,
    ));
  }

  it("enables importing a resource link and immediately opens the resulting Zotero entry", async () => {
    await render();
    const button = container.querySelector<HTMLButtonElement>('button[title="Send this link to Zotero"]')!;
    expect(button.disabled).toBe(false);
    vi.mocked(api.zoteroSendTopic).mockResolvedValue({ created: ["LINK0001"], failures: [], collection_key: null, up_to_date: 0 });
    const url = "zotero://select/library/items/LINK0001";
    vi.mocked(api.listItems).mockResolvedValue([{ ...resource, zotero_url: url }]);
    await act(async () => button.click());
    expect(api.zoteroSendTopic).toHaveBeenCalledWith("course", "123");
    await act(async () => container.querySelector<HTMLAnchorElement>('a[title="Open in Zotero"]')!.click());
    expect(api.openUrl).toHaveBeenCalledWith(url);
    expect(api.downloadTopicFile).not.toHaveBeenCalled();
  });

  it.each([
    { ...resource, item_type: "Quiz" },
    { ...resource, url: "/d2l/common/dialogs/quickLink?type=quiz" },
    { ...resource, url: "javascript:alert(1)" },
    { ...resource, url: null },
  ])("keeps unsupported activities and invalid links disabled: $url", async (item) => {
    await render(item);
    expect(container.querySelector<HTMLButtonElement>('button[title="This course activity cannot be sent to Zotero"]')?.disabled).toBe(true);
    expect(api.zoteroSendTopic).not.toHaveBeenCalled();
  });
});
