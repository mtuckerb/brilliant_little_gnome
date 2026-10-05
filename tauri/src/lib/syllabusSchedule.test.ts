import { describe, expect, it } from "vitest";
import { extractSyllabusSchedule } from "./syllabusSchedule";

describe("syllabus meeting schedule", () => {
  it.each([
    ["Class meets: MWF 9:00–9:50 AM in Room 214", { days: "Monday, Wednesday, Friday", time: "9:00–9:50 AM" }],
    ["Class days: Tuesdays and Thursdays\nMeeting time: 1:30 to 2:45 PM", { days: "Tuesday, Thursday", time: "1:30 to 2:45 PM" }],
    ["Meeting schedule:\nTuTh 09:00-10:15", { days: "Tuesday, Thursday", time: "09:00-10:15" }],
    ["Class days: M / W / F\nTime: 9:00-9:50 AM", { days: "Monday, Wednesday, Friday", time: "9:00-9:50 AM" }],
    ["Class days: Monday through Friday", { days: "Monday, Tuesday, Wednesday, Thursday, Friday", time: null }],
    ["Office hours:\nDays: Tuesdays\nTime: 9:00-10:00 AM", { days: null, time: null }],
    ["Days: Mon/Wed\nTime: 10 AM - 11:15 AM ET", { days: "Monday, Wednesday", time: "10 AM - 11:15 AM ET" }],
    ["Class meets Mondays from 9 AM to 10 AM", { days: "Monday", time: "9 AM to 10 AM" }],
    ["Lecture: MWF 9:00–9:50 AM\nLab: Friday 2:00–4:00 PM", { days: "Lecture: Monday, Wednesday, Friday; Lab: Friday", time: "Lecture: 9:00–9:50 AM; Lab: 2:00–4:00 PM" }],
    ["Office hours: Tuesdays 9:30–10:45 AM\nClass days: MWF\nClass time: 11:00–11:50 AM", { days: "Monday, Wednesday, Friday", time: "11:00–11:50 AM" }],
    ["Class days: Monday\nTextbook shipping available Fridays at 5 PM", { days: "Monday", time: null }],
    ["Office hours: Tuesday 9 AM\nLecture: MWF 10:00–10:50 AM", { days: "Lecture: Monday, Wednesday, Friday", time: "Lecture: 10:00–10:50 AM" }],
    ["Office hours:\nTuesday 9 AM–10 AM", { days: null, time: null }],
    ["Exam: Friday 9:00 AM\nAssignments due Mondays at 11:59 PM", { days: null, time: null }],
    ["Room: 214\n3 credits\nFall 2026", { days: null, time: null }],
    ["Class days: Wednesday\nNo meeting time listed", { days: "Wednesday", time: null }],
  ])("extracts recurring meetings from %s", (text, expected) => {
    expect(extractSyllabusSchedule(text)).toEqual(expected);
  });
});
