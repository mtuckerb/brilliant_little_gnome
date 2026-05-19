import { unzipSync, strFromU8 } from "fflate";

const DOCX_XML = /^word\/document\.xml$/;
const XLSX_XML = /^xl\/worksheets\/sheet\d+\.xml$/;
const PPTX_XML = /^ppt\/slides\/slide\d+\.xml$/;
const SHARED_STRINGS = "xl/sharedStrings.xml";
const MAX_OFFICE_ENTRIES = 800;
const MAX_OFFICE_INFLATED_BYTES = 30 * 1024 * 1024;
const MAX_OFFICE_TEXT_CHARS = 2 * 1024 * 1024;

function assertOfficeZipBounds(files: Record<string, Uint8Array>) {
  const entries = Object.entries(files);
  if (entries.length > MAX_OFFICE_ENTRIES) {
    throw new Error("Office document is too complex to preview safely — open externally.");
  }
  const inflatedBytes = entries.reduce((sum, [, data]) => sum + data.byteLength, 0);
  if (inflatedBytes > MAX_OFFICE_INFLATED_BYTES) {
    throw new Error("Office document is too large to decompress safely — open externally.");
  }
}

function truncatePreview(text: string): string {
  if (text.length <= MAX_OFFICE_TEXT_CHARS) return text;
  return text.slice(0, MAX_OFFICE_TEXT_CHARS) + "\n\n[Preview truncated — open externally to view the full document.]";
}

function xmlText(xml: string): string {
  return xml
    .replace(/<\/?(w:p|a:p|row)[^>]*>/g, "\n")
    .replace(/<[^>]+>/g, " ")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/[ \t]+/g, " ")
    .replace(/\n\s+/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function sharedStrings(files: Record<string, Uint8Array>): string[] {
  const shared = files[SHARED_STRINGS];
  if (!shared) return [];
  const xml = strFromU8(shared);
  return Array.from(xml.matchAll(/<si[^>]*>([\s\S]*?)<\/si>/g)).map((m) => xmlText(m[1] ?? ""));
}

function sheetText(xml: string, strings: string[]): string {
  return Array.from(xml.matchAll(/<c[^>]*?(?:t="([^"]+)")?[^>]*>([\s\S]*?)<\/c>/g))
    .map((m) => {
      const type = m[1];
      const cell = m[2] ?? "";
      const value = /<v[^>]*>([\s\S]*?)<\/v>/.exec(cell)?.[1] ?? xmlText(cell);
      return type === "s" ? strings[Number(value)] ?? value : value;
    })
    .filter(Boolean)
    .join("\t");
}

export function extractOfficeText(bytes: Uint8Array, extension: string): string {
  const files = unzipSync(bytes);
  assertOfficeZipBounds(files);
  const entries = Object.entries(files).sort(([a], [b]) => a.localeCompare(b));
  if (extension === "xlsx") {
    const strings = sharedStrings(files);
    return truncatePreview(entries.filter(([name]) => XLSX_XML.test(name)).map(([name, data]) => `# ${name}\n${sheetText(strFromU8(data), strings)}`).join("\n\n"));
  }
  const pattern = extension === "docx" ? DOCX_XML : PPTX_XML;
  return truncatePreview(entries.filter(([name]) => pattern.test(name)).map(([, data]) => xmlText(strFromU8(data))).filter(Boolean).join("\n\n---\n\n"));
}
