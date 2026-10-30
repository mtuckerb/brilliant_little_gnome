// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api";
import RichText from "./RichText";

vi.mock("../api", () => ({ api: { openUrl: vi.fn(async () => {}) } }));

describe("Zotero source links in task notes", () => {
  let container: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    vi.clearAllMocks();
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  it("renders and opens a generated local Zotero document link", async () => {
    const url = "zotero://open-pdf/library/items/ABCD1234";
    act(() => root.render(<RichText content={`From module **Readings**\n\n[Chapter 4](<${url}>)`} />));
    const link = container.querySelector("a")!;
    expect(link.getAttribute("href")).toBe(url);
    expect(container.querySelector("strong")?.textContent).toBe("Readings");
    await act(async () => link.click());
    expect(api.openUrl).toHaveBeenCalledWith(url);
  });

  it("continues rendering Brightspace source links as markdown", () => {
    act(() => root.render(<RichText content="[Reading](<https://school.test/reading>)" />));
    expect(container.querySelector("a")?.href).toBe("https://school.test/reading");
  });

  it("does not allow arbitrary custom protocols or Zotero actions", () => {
    act(() => root.render(<RichText content="[Bad](zotero://delete/library/items/ABCD1234) [Unsafe](javascript:alert)" />));
    for (const link of container.querySelectorAll("a")) expect(link.getAttribute("href")).toBe("");
  });

  it.each(["markdown", "html"] as const)("resolves existing %s source anchors without rewriting images", async (format) => {
    const source = "https://school.test/reading";
    const destination = "zotero://open-pdf/library/items/ABCD1234";
    const content = format === "html"
      ? `<p><a href="${source}">Reading</a><img src="${source}" /></p>`
      : `[Reading](${source}) ![Cover](${source})`;
    act(() => root.render(<RichText content={content} format={format} resolveLink={(url) => url === source ? destination : url} />));
    const link = container.querySelector("a")!;
    expect(link.getAttribute("href")).toBe(destination);
    expect(container.querySelector("img")?.getAttribute("src")).toBe(source);
    await act(async () => link.click());
    expect(api.openUrl).toHaveBeenCalledWith(destination);
  });
});
