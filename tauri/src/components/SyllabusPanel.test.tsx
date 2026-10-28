// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, onAppEvent, type AppEvent } from "../api";
import SyllabusPanel from "./SyllabusPanel";

vi.mock("../api", () => ({ onAppEvent: vi.fn(async () => () => {}), api: {
  getCourseOverview: vi.fn(), openUrl: vi.fn(async () => {}),
  zoteroSendSyllabus: vi.fn(), fetchCourseOverviewAttachment: vi.fn(),
} }));
vi.mock("./ToastProvider", () => ({ useToast: () => ({ show: vi.fn() }) }));

describe("syllabus Zotero destination", () => {
  let container: HTMLDivElement;
  let root: Root;
  const meta = {
    description_html: null, has_attachment: true,
    attachment_name: "Syllabus.pdf", attachment_url: null,
  };
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    vi.resetAllMocks();
    vi.mocked(onAppEvent).mockResolvedValue(() => {});
    vi.mocked(api.openUrl).mockResolvedValue();
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  it("opens an already synced syllabus in Zotero", async () => {
    const url = "zotero://open-pdf/library/items/ABCD1234";
    vi.mocked(api.getCourseOverview).mockResolvedValue({ ...meta, zotero_url: url });
    await act(async () => root.render(<SyllabusPanel courseId="course" />));
    const button = Array.from(container.querySelectorAll("button"))
      .find((b) => b.textContent === "Open in Zotero")!;
    await act(async () => button.click());
    expect(api.openUrl).toHaveBeenCalledWith(url);
    expect(api.fetchCourseOverviewAttachment).not.toHaveBeenCalled();
  });

  it("updates the destination immediately after sending to Zotero", async () => {
    const url = "https://www.zotero.org/users/42/items/ABCD1234";
    vi.mocked(api.getCourseOverview).mockResolvedValueOnce(meta)
      .mockResolvedValueOnce({ ...meta, zotero_url: url });
    vi.mocked(api.zoteroSendSyllabus).mockResolvedValue({
      created: ["ABCD1234"], failures: [], collection_key: null, up_to_date: 0,
    });
    await act(async () => root.render(<SyllabusPanel courseId="course" />));
    const send = container.querySelector<HTMLButtonElement>('button[title="Send the syllabus PDF to your Zotero library"]')!;
    await act(async () => send.click());
    expect(api.zoteroSendSyllabus).toHaveBeenCalledWith("course");
    expect(container.textContent).toContain("Open in Zotero");
    const open = Array.from(container.querySelectorAll("button"))
      .find((b) => b.textContent === "Open in Zotero")!;
    await act(async () => open.click());
    expect(api.openUrl).toHaveBeenCalledWith(url);
  });

  it("recovers an old syllabus link on the open page without sending it again", async () => {
    let onEvent: ((event: AppEvent) => void) | undefined;
    vi.mocked(onAppEvent).mockImplementationOnce(async (callback) => { onEvent = callback; return () => {}; });
    vi.mocked(api.getCourseOverview).mockResolvedValue(meta);
    await act(async () => root.render(<SyllabusPanel courseId="course" />));
    expect(container.textContent).not.toContain("Open in Zotero");
    vi.mocked(api.getCourseOverview).mockResolvedValue({ ...meta, zotero_url: "zotero://open-pdf/library/items/ABCD1234" });
    await act(async () => onEvent!({ kind: "zotero_links_updated" }));
    expect(container.textContent).toContain("Open in Zotero");
    expect(api.zoteroSendSyllabus).not.toHaveBeenCalled();
  });
});
