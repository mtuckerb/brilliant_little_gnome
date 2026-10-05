import { extractSyllabusSchedule } from "./syllabusSchedule";
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

async function attachmentText(file: { bytes_base64: string; mime: string | null; filename: string }): Promise<string | null> {
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
  return text;
}

export interface CourseMeetingInfo {
  room?: string;
  roomSource?: string;
  days?: string;
  time?: string;
  daysSource?: string;
  timeSource?: string;
  error?: string;
  readable?: boolean;
}

async function readCourseMeetingInfo(courseId: string, roomOnly = false): Promise<CourseMeetingInfo> {
  const result: CourseMeetingInfo = {};
  let failed = false;
  const read = (text: string | null, source: string) => {
    if (!text?.trim()) return;
    result.readable = true;
    const room = extractSyllabusRoom(text);
    if (!result.room && room) { result.room = room; result.roomSource = source; }
    if (!roomOnly) {
      const schedule = extractSyllabusSchedule(text);
      if (!result.days && schedule.days) { result.days = schedule.days; result.daysSource = source; }
      if (!result.time && schedule.time) { result.time = schedule.time; result.timeSource = source; }
    }
  };
  const complete = () => result.room && (roomOnly || (result.days && result.time));
  try {
    const overview = await api.getCourseOverview(courseId);
    if (overview.description_html) read(syllabusHtmlText(overview.description_html), "Course overview");
    if (complete()) return result;
    if (overview.has_attachment) {
      try {
        const file = await api.fetchCourseOverviewAttachment(courseId);
        read(await attachmentText(file), file.filename);
        if (complete()) return result;
      } catch { failed = true; }
    }
  } catch { /* Some courses have no overview. Continue to the syllabus in Modules. */ }
  try {
    const items = await api.listCourseItems(courseId);
    const syllabi = items.filter((item) => /\bsyllabus\b|\bsyllabi\b|course\s+overview/i.test(item.title) && !item.is_hidden);
    for (const item of syllabi) {
      try {
        const file = await api.previewTopicFile(courseId, item.brightspace_id);
        read(await attachmentText(file), item.title);
        if (complete()) return result;
      } catch { failed = true; }
    }
  } catch { failed = true; }
  // Keep partial information when another source is unavailable.
  if (failed) result.error = "Could not read the syllabus. Sync this course to try again.";
  return result;
}

export function findCourseMeetingInfo(courseId: string): Promise<CourseMeetingInfo> {
  return readCourseMeetingInfo(courseId);
}

export async function findCourseRoom(courseId: string): Promise<SyllabusRoom | null> {
  const info = await readCourseMeetingInfo(courseId, true);
  if (info.room) return { room: info.room, source: info.roomSource! };
  if (info.error) throw new Error(info.error);
  return null;
}

export function parseCourseMeetingInfo(raw: string | null | undefined): CourseMeetingInfo | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw);
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const info: CourseMeetingInfo = {};
    for (const key of ["room", "roomSource", "days", "daysSource", "time", "timeSource"] as const) {
      if (typeof value[key] === "string" && value[key].trim()) info[key] = value[key];
    }
    return info;
  } catch { return null; }
}
