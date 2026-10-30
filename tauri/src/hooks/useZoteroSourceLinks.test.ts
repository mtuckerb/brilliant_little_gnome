import { describe, expect, it } from "vitest";
import type { ContentItem } from "../types";
import { zoteroSourceResolver } from "./useZoteroSourceLinks";

const target = "zotero://open-pdf/library/items/8EEJQ897";
const item = { url: "/content/Kanel Ch. 1.pdf", brightspace_id: "123", zotero_url: target } as ContentItem;

describe("course source matching", () => {
  const resolve = zoteroSourceResolver([item], "school.test", "course");
  it.each([
    "https://school.test/content/Kanel%20Ch.%201.pdf",
    "/content/Kanel Ch. 1.pdf",
    "school.test/content/Kanel%20Ch.%201.pdf",
    "https://school.test/content/Kanel%20Ch.%201.pdf#page=1",
    "https://school.test/d2l/le/content/course/viewContent/123/View",
  ])("recognizes a saved source: %s", (source) => expect(resolve(source)).toBe(target));
  it.each([
    "https://other.test/content/Kanel%20Ch.%201.pdf",
    "/content/Another.pdf",
    "https://school.test/d2l/le/content/other/viewContent/123/View",
    "javascript:alert(1)",
  ])("preserves unrelated URLs: %s", (source) => expect(resolve(source)).toBe(source));
  it("does not guess between conflicting mappings", () => {
    const source = "/content/Kanel Ch. 1.pdf";
    const ambiguous = zoteroSourceResolver([item, { ...item, zotero_url: "zotero://open-pdf/library/items/OTHER123" }], "school.test", "course");
    expect(ambiguous(source)).toBe(source);
  });
});
