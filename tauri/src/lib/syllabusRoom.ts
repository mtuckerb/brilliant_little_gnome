import { api } from "../api";
import { base64ToBytes, extensionForFile, PREVIEW_MAX_BYTES } from "./fileViewer";
import { extractOfficeText } from "./officeText";

export interface SyllabusRoom {
  room: string;
  source: string;
}

export function syllabusHtmlText(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  doc.querySelectorAll("script, style").forEach((node) => node.remove());
  doc.querySelectorAll("br, p, div, tr, li, h1, h2, h3, h4").forEach((node) => {
    node.append(doc.createTextNode("\n"));
  });
  doc.querySelectorAll("td, th").forEach((node) => node.append(doc.createTextNode(" ")));
  return doc.body.textContent ?? "";
}

// Require an explicit room/location label; dates, course codes and instructor
// offices elsewhere in the syllabus must not become the class's room number.
export function extractSyllabusRoom(text: string): string | null {
  const lines = text.replace(/\r/g, "").replace(/\u00a0/g, " ").split("\n")
    .map((line) => line.replace(/[\t ]+/g, " ").trim()).filter(Boolean);
  const candidates: { room: string; score: number }[] = [];
  const label = /\b(class\s*(?:room|location)|(?:class|lecture|lab|meeting|course)\s+(?:location|room|meets(?:\s+in)?)|location|room(?:\s+(?:number|no\.?))?|rm\.?)\s*[:#–—-]?\s*/i;
  const number = /\b[A-Z]?\d{1,4}[A-Z]?\b(?!\s*[:/\d])/i;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const match = label.exec(line);
    if (!match) continue;
    const prefix = line.slice(0, match.index);
    if (/office|instructor|professor|faculty|staff|contact/i.test(prefix)) continue;
    if (/^(?:room|rm)/i.test(match[1]) && i > 0 && /office\s*(?:location)?\s*:?$/i.test(lines[i - 1])) continue;
    let value = line.slice(match.index + match[0].length);
    if (!value && lines[i + 1]) value = lines[i + 1];
    if (/meets/i.test(match[1])) value = value.replace(/^.*?\b(?:in|at)\s+/i, "");
    value = value.split(/\s*(?:;|\||\b(?:instructor|professor|email|office|phone|class time|meeting time|office hours)\s*:)/i)[0].trim();
    if (/^(?:online|remote|virtual|zoom)\b/i.test(value)) {
      candidates.push({ room: "Online", score: 80 });
      continue;
    }
    const roomNumber = number.exec(value);
    if (!roomNumber || value.length > 120) continue;
    // A time or date is not a room (including the trailing minutes in 9:30).
    if (/[:/]/.test(value.slice(0, roomNumber.index))) continue;
    const end = roomNumber.index + roomNumber[0].length;
    const location = value.slice(0, end).replace(/^(?:in|at)\s+/i, "").trim();
    if (/\b(?:credits?|units?|week|chapter|page|am|pm)\b/i.test(location)) continue;
    const after = value.slice(end);
    if (/^(?:room|rm)/i.test(match[1]) && (
      /\b(?:reserve|study|available|policy)\b/i.test(prefix)
      || /^\s+(?:is|are|for|must|may|should|can)\b/i.test(after)
    )) continue;
    // Also preserve a building placed after the room: "204, Science Hall".
    const building = /^\s*,\s*([A-Za-z][A-Za-z .'-]*?(?:Hall|Building|Center|Centre))\b/i.exec(after)?.[1];
    const score = /^(?:class|lecture|lab|meeting|course)/i.test(match[1]) ? 100
      : /^location/i.test(match[1]) ? 80 : 50;
    candidates.push({ room: building ? `${location}, ${building}` : location, score });
  }
  candidates.sort((a, b) => b.score - a.score);
  return candidates[0]?.room ?? null;
}

async function attachmentRoom(file: { bytes_base64: string; mime: string | null; filename: string }): Promise<string | null> {
  // Check before decoding the backend response as well as before PDF parsing.
  if (file.bytes_base64.length > Math.ceil(PREVIEW_MAX_BYTES / 3) * 4) return null;
  const bytes = base64ToBytes(file.bytes_base64);
  const ext = extensionForFile(file.filename);
  let text: string;
  if (ext === "pdf" || file.mime?.includes("pdf") || new TextDecoder().decode(bytes.slice(0, 5)) === "%PDF-") {
    const { extractPdfText } = await import("./syllabusPdf");
    text = await extractPdfText(bytes);
  } else if (ext === "docx") {
    text = extractOfficeText(bytes, "docx");
  } else if (["html", "htm"].includes(ext) || file.mime?.includes("html")) {
    text = syllabusHtmlText(new TextDecoder().decode(bytes));
  } else if (["txt", "md", "markdown"].includes(ext) || file.mime?.startsWith("text/plain")) {
    text = new TextDecoder().decode(bytes);
  } else {
    return null;
  }
  return extractSyllabusRoom(text);
}

export async function findCourseRoom(courseId: string): Promise<SyllabusRoom | null> {
  // Some instructors attach the syllabus to Overview; others put it in Modules.
  // A missing overview or unreadable file should not stop the other sources.
  let failed = false;
  try {
    const overview = await api.getCourseOverview(courseId);
    if (overview.description_html) {
      const room = extractSyllabusRoom(syllabusHtmlText(overview.description_html));
      if (room) return { room, source: "Course overview" };
    }
    if (overview.has_attachment) {
      try {
        const file = await api.fetchCourseOverviewAttachment(courseId);
        const room = await attachmentRoom(file);
        if (room) return { room, source: file.filename };
      } catch { failed = true; }
    }
  } catch { /* A course may have no overview at all. */ }
  const items = await api.listCourseItems(courseId);
  const syllabi = items.filter((item) => /\bsyllabus\b|\bsyllabi\b|course\s+overview/i.test(item.title) && !item.is_hidden);
  for (const item of syllabi) {
    try {
      const file = await api.previewTopicFile(courseId, item.brightspace_id);
      const room = await attachmentRoom(file);
      if (room) return { room, source: item.title };
    } catch { failed = true; }
  }
  if (failed) throw new Error("Could not read the syllabus. Try syncing this course again.");
  return null;
}
