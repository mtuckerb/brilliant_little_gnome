import { getDocument, GlobalWorkerOptions } from "pdfjs-dist/legacy/build/pdf.mjs";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";

GlobalWorkerOptions.workerSrc = workerUrl;

export async function extractPdfText(bytes: Uint8Array): Promise<string> {
  const task = getDocument({ data: bytes });
  try {
    const pdf = await task.promise;
    const pages: string[] = [];
    // Syllabus locations normally appear in the opening pages. Bound work for
    // an unexpectedly large file without blocking the rest of the overview.
    for (let pageNumber = 1; pageNumber <= Math.min(pdf.numPages, 20); pageNumber += 1) {
      const page = await pdf.getPage(pageNumber);
      const content = await page.getTextContent();
      let previousY: number | null = null;
      let text = "";
      for (const item of content.items) {
        if (!("str" in item)) continue;
        const y = item.transform[5];
        if (previousY !== null && Math.abs(y - previousY) > 3) text += "\n";
        text += `${item.str}${item.hasEOL ? "\n" : " "}`;
        previousY = y;
      }
      pages.push(text);
      page.cleanup();
    }
    return pages.join("\n");
  } finally {
    await task.destroy();
  }
}
