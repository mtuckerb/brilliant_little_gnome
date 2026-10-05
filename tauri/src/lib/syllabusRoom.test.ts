// @vitest-environment jsdom
import { describe, expect, it, vi, beforeEach } from "vitest";
import { api } from "../api";
import { extractSyllabusRoom, findCourseRoom, syllabusHtmlText } from "./syllabusRoom";

vi.mock("../api", () => ({ api: {
  getCourseOverview: vi.fn(), fetchCourseOverviewAttachment: vi.fn(),
  listCourseItems: vi.fn(), previewTopicFile: vi.fn(),
} }));

describe("syllabus room extraction", () => {
  it.each([
    ["Classroom: 214", "214"],
    ["Class location: Payson Smith Hall 305", "Payson Smith Hall 305"],
    ["Location: Luther Bonney, Room 301", "Luther Bonney, Room 301"],
    ["Room number: B204", "B204"],
    ["Room: 204, Science Hall", "204, Science Hall"],
    ["Meeting location:\nWishcamper 104\nInstructor: Ada", "Wishcamper 104"],
    ["Office: Room 206\nClassroom: 214", "214"],
    ["Office location:\nRoom 206\nLocation: Science Hall 202", "Science Hall 202"],
    ["Office: Room 206\nOffice hours: Tuesdays at 9:30", null],
    ["Course: BIO 214\n3 credits\nMeeting time: 9:30", null],
    ["Location: 9:30 AM", null],
    ["Location: Online via Zoom", "Online"],
    ["Room 102 is available for studying.\nClassroom: 214", "214"],
    ["Room 102 is available for studying.", null],
    ["Please reserve room 102 for group work.", null],
    ["Class meets Tuesdays 9:30-10:45 in Science Hall, Room 214", "Science Hall, Room 214"],
  ])("reads %s", (text, room) => {
    expect(extractSyllabusRoom(text)).toBe(room);
  });

  it("preserves HTML table labels and line breaks, excluding scripts", () => {
    const text = syllabusHtmlText('<script>Classroom: 666</script><table><tr><td>Office:</td><td>Room 206</td></tr><tr><td>Class location:</td><td>Payson Smith Hall 305</td></tr></table>');
    expect(extractSyllabusRoom(text)).toBe("Payson Smith Hall 305");
  });
});

describe("syllabus sources", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.listCourseItems).mockResolvedValue([]);
  });

  it("uses the overview description without downloading a file", async () => {
    vi.mocked(api.getCourseOverview).mockResolvedValue({ description_html: "<p>Classroom: 214</p>", has_attachment: true, attachment_name: null, attachment_url: null });
    expect(await findCourseRoom("a")).toEqual({ room: "214", source: "Course overview" });
    expect(api.fetchCourseOverviewAttachment).not.toHaveBeenCalled();
  });

  it("finds a syllabus in Modules when the overview is missing", async () => {
    vi.mocked(api.getCourseOverview).mockRejectedValue(new Error("404"));
    vi.mocked(api.listCourseItems).mockResolvedValue([
      { id: 1, module_id: "m", brightspace_id: "topic", title: "Fall Syllabus", item_type: "File", url: null, is_hidden: false, sort_order: 0 },
    ]);
    vi.mocked(api.previewTopicFile).mockResolvedValue({ bytes_base64: btoa("Classroom: Science Hall 204"), mime: "text/plain", filename: "syllabus.txt" });
    expect(await findCourseRoom("b")).toEqual({ room: "Science Hall 204", source: "Fall Syllabus" });
    expect(api.previewTopicFile).toHaveBeenCalledWith("b", "topic");
  });

  it("continues to Modules when the attached syllabus cannot be read", async () => {
    vi.mocked(api.getCourseOverview).mockResolvedValue({ description_html: null, has_attachment: true, attachment_name: null, attachment_url: null });
    vi.mocked(api.fetchCourseOverviewAttachment).mockRejectedValue(new Error("Offline"));
    await expect(findCourseRoom("a")).rejects.toThrow("Could not read the syllabus");
    expect(api.listCourseItems).toHaveBeenCalledWith("a");
  });

  it("reports an absent room without inventing one", async () => {
    vi.mocked(api.getCourseOverview).mockResolvedValue({ description_html: "<p>Office: Room 206</p>", has_attachment: false, attachment_name: null, attachment_url: null });
    expect(await findCourseRoom("a")).toBeNull();
  });
});

describe("shared room and schedule reader", () => {
  beforeEach(() => { vi.resetAllMocks(); vi.mocked(api.listCourseItems).mockResolvedValue([]); });
  it("reads a syllabus once for all three fields", async () => {
    const { findCourseMeetingInfo } = await import("./syllabusRoom");
    vi.mocked(api.getCourseOverview).mockResolvedValue({ description_html: null, has_attachment: true, attachment_name: null, attachment_url: null });
    vi.mocked(api.fetchCourseOverviewAttachment).mockResolvedValue({ filename: "syllabus.txt", mime: "text/plain", bytes_base64: btoa("Classroom: 214\nClass meets: MWF 9:00-9:50 AM") });
    expect(await findCourseMeetingInfo("a")).toEqual({ room: "214", roomSource: "syllabus.txt", days: "Monday, Wednesday, Friday", daysSource: "syllabus.txt", time: "9:00-9:50 AM", timeSource: "syllabus.txt" });
    expect(api.fetchCourseOverviewAttachment).toHaveBeenCalledTimes(1);
    expect(api.listCourseItems).not.toHaveBeenCalled();
  });
  it("fills a missing schedule from Modules while preserving the overview room", async () => {
    const { findCourseMeetingInfo } = await import("./syllabusRoom");
    vi.mocked(api.getCourseOverview).mockResolvedValue({ description_html: "<p>Classroom: 214</p>", has_attachment: false, attachment_name: null, attachment_url: null });
    vi.mocked(api.listCourseItems).mockResolvedValue([{ id: 1, module_id: "m", brightspace_id: "s", title: "Syllabus", item_type: "File", url: null, is_hidden: false, sort_order: 0 }]);
    vi.mocked(api.previewTopicFile).mockResolvedValue({ filename: "syllabus.txt", mime: "text/plain", bytes_base64: btoa("Classroom: 999\nClass days: TR\nMeeting time: 10:00-11:15 AM") });
    expect(await findCourseMeetingInfo("a")).toMatchObject({ room: "214", roomSource: "Course overview", days: "Tuesday, Thursday", time: "10:00-11:15 AM" });
  });
  it("keeps a room even when the remaining syllabus is unavailable", async () => {
    const { findCourseMeetingInfo } = await import("./syllabusRoom");
    vi.mocked(api.getCourseOverview).mockResolvedValue({ description_html: "<p>Classroom: 214</p>", has_attachment: true, attachment_name: null, attachment_url: null });
    vi.mocked(api.fetchCourseOverviewAttachment).mockRejectedValue(new Error("offline"));
    expect(await findCourseMeetingInfo("a")).toMatchObject({ room: "214", error: expect.stringContaining("Could not read") });
  });
});
