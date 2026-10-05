import { describe, expect, it, vi } from "vitest";
import { extractPdfText } from "./syllabusPdf";
import { extractSyllabusRoom } from "./syllabusRoom";

// Node uses PDF.js's fake worker, loaded from the installed worker module.
vi.mock("pdfjs-dist/legacy/build/pdf.worker.min.mjs?url", () => ({
  default: new URL("../../node_modules/pdfjs-dist/legacy/build/pdf.worker.min.mjs", import.meta.url).href,
}));

function syllabusPdf(): Uint8Array {
  const stream = "BT /F1 12 Tf 72 720 Td (Office: Room 206) Tj 0 -20 Td (Classroom: Science Hall 214) Tj ET";
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`,
  ];
  let pdf = "%PDF-1.4\n";
  const offsets = [0];
  objects.forEach((obj, index) => {
    offsets.push(pdf.length);
    pdf += `${index + 1} 0 obj\n${obj}\nendobj\n`;
  });
  const xref = pdf.length;
  pdf += `xref\n0 6\n0000000000 65535 f \n`;
  pdf += offsets.slice(1).map((offset) => `${String(offset).padStart(10, "0")} 00000 n \n`).join("");
  pdf += `trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF`;
  return new TextEncoder().encode(pdf);
}

describe("PDF syllabus text", () => {
  it("reads real PDF bytes and keeps the instructor office separate from the classroom", async () => {
    const text = await extractPdfText(syllabusPdf());
    expect(text).toContain("Classroom: Science Hall 214");
    expect(extractSyllabusRoom(text)).toBe("Science Hall 214");
  });

  it("rejects an invalid PDF", async () => {
    await expect(extractPdfText(new TextEncoder().encode("not a pdf"))).rejects.toThrow();
  });
});
