import { describe, expect, it } from "vitest";
import { PREVIEW_MAX_BYTES, extensionForFile, routeFileToViewer } from "./fileViewer";

describe("file viewer routing", () => {
  it("routes accepted extensions to the in-app viewer", () => {
    for (const ext of ["pdf", "doc", "docx", "xls", "xlsx", "csv", "md", "markdown", "ppt", "pptx", "rtf"]) {
      expect(routeFileToViewer(`lecture.${ext}`, 1024).supported).toBe(true);
    }
  });

  it("falls back for unsupported extensions", () => {
    expect(routeFileToViewer("archive.zip", 1024)).toMatchObject({ supported: false, reason: "unsupported" });
  });

  it("enforces the 25 MB preview cap", () => {
    expect(routeFileToViewer("slides.pdf", PREVIEW_MAX_BYTES).supported).toBe(true);
    expect(routeFileToViewer("slides.pdf", PREVIEW_MAX_BYTES + 1)).toMatchObject({ supported: false, reason: "too_large" });
  });

  it("extracts extensions from URLs with query strings", () => {
    expect(extensionForFile("https://example.invalid/path/File.PDF?download=1")).toBe("pdf");
  });
});
