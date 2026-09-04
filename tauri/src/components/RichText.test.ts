import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import RichText, { looksLikeHtml } from "./RichText";

describe("looksLikeHtml", () => {
  it("recognizes opening HTML tags with or without attributes", () => {
    expect(looksLikeHtml("<p>Instructor notes</p>")).toBe(true);
    expect(looksLikeHtml('<div class="notice">Read this</div>')).toBe(true);
  });

  it("does not mistake a Markdown angle-bracket link destination for HTML", () => {
    expect(looksLikeHtml("[Reading](<https://example.edu/path>)")).toBe(false);
  });

  it("renders generated task-note links as clickable anchors in auto mode", () => {
    const markup = renderToStaticMarkup(
      createElement(RichText, { content: "[Reading](<https://example.edu/path>)" }),
    );
    expect(markup).toContain('<a href="https://example.edu/path">Reading</a>');
  });
});
