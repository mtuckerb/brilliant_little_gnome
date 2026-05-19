import { describe, expect, it } from "vitest";
import { zipSync, strToU8 } from "fflate";
import { extractOfficeText } from "./officeText";

describe("Office Open XML text extraction", () => {
  it("extracts readable docx text", () => {
    const bytes = zipSync({ "word/document.xml": strToU8("<w:document><w:p><w:r><w:t>Hello class</w:t></w:r></w:p></w:document>") });
    expect(extractOfficeText(bytes, "docx")).toContain("Hello class");
  });

  it("rejects documents with excessive inflated size", () => {
    const bytes = zipSync({ "word/document.xml": new Uint8Array(31 * 1024 * 1024) });
    expect(() => extractOfficeText(bytes, "docx")).toThrow(/too large to decompress safely/);
  });
});
