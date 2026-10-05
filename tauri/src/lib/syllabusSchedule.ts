export interface SyllabusSchedule {
  days: string | null;
  time: string | null;
}

const dayPattern = /\b(?:mon(?:day)?s?|tue(?:s(?:day)?)?s?|wed(?:nesday)?s?|thu(?:rs(?:day)?)?s?|fri(?:day)?s?|sat(?:urday)?s?|sun(?:day)?s?)\b\.?/gi;
const dayNames: Record<string, string> = { mon: "Monday", tue: "Tuesday", wed: "Wednesday", thu: "Thursday", fri: "Friday", sat: "Saturday", sun: "Sunday" };
const shortDays: Record<string, string> = { M: "Monday", T: "Tuesday", W: "Wednesday", R: "Thursday", F: "Friday", S: "Saturday", U: "Sunday" };
const clock = String.raw`(?:[01]?\d|2[0-3])(?::[0-5]\d)?\s*(?:a\.?m\.?|p\.?m\.?)?`;
const timePattern = new RegExp(String.raw`\b(${clock})\s*(?:[-–—]|\bto\b)\s*(${clock})(?![\d:])(?:\s*(?:ET|EST|EDT|CT|CST|CDT|MT|MST|MDT|PT|PST|PDT))?`, "i");
const singleTime = /\b(?:[01]?\d|2[0-3]):[0-5]\d\s*(?:a\.?m\.?|p\.?m\.?)?\b|\b(?:1[0-2]|[1-9])\s*(?:a\.?m\.?|p\.?m\.?)\b/i;

function daysIn(text: string): string | null {
  const matches = [...text.matchAll(dayPattern)];
  const found: string[] = [];
  const week = Object.values(dayNames);
  for (let i = 0; i < matches.length; i += 1) {
    const name = dayNames[matches[i][0].slice(0, 3).toLowerCase()];
    found.push(name);
    const next = matches[i + 1];
    if (next && /^\s*(?:[-–—]|to|through)\s*$/i.test(text.slice(matches[i].index! + matches[i][0].length, next.index))) {
      const end = week.indexOf(dayNames[next[0].slice(0, 3).toLowerCase()]);
      if (end !== week.indexOf(name)) {
        for (let n = (week.indexOf(name) + 1) % 7; n !== end; n = (n + 1) % 7) found.push(week[n]);
      }
    }
  }
  if (!found.length) {
    for (const compact of text.matchAll(/\b(?:MWF|MW|MF|WF|TR|TTh|TuTh|MTWRF|Th|Tu|[MTWRFSU])\b/g)) {
      const codes = compact[0].replace(/Tu/g, "T").replace(/Th/g, "R");
      found.push(...[...codes].map((code) => shortDays[code]));
    }
  }
  return found.length ? [...new Set(found)].join(", ") : null;
}

function timeIn(text: string): string | null {
  const range = timePattern.exec(text)?.[0].trim();
  // Bare numbers such as credits or dates do not establish a meeting time.
  if (range && /:|[ap]\.?m/i.test(range)) return range.replace(/\s+/g, " ");
  return singleTime.exec(text)?.[0].trim().replace(/\s+/g, " ") ?? null;
}

export function extractSyllabusSchedule(text: string): SyllabusSchedule {
  const lines = text.replace(/\r|\u00a0/g, " ").split("\n").map((line) => line.replace(/[\t ]+/g, " ").trim()).filter(Boolean);
  const days: string[] = [];
  const times: string[] = [];
  let officeSection = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const meetingLabel = /\b(?:(?:class|course|lecture|lab|meeting)\s+(?:days?|times?|schedule|meet(?:s|ings)?|hours?)|(?:lecture|lab|days?(?:\s+of\s+(?:the\s+)?week)?|time|schedule)\s*:)/i.test(line);
    if (/\b(?:office|advising|tutoring)\s*(?:hours|schedule|availability)\b/i.test(line)) { officeSection = true; continue; }
    if (/\b(?:class|course|meeting)\s+(?:days?|times?|schedule|meet(?:s|ings)?|hours?)|^\s*(?:lecture|lab)\b/i.test(line)) officeSection = false;
    if (officeSection || /\b(?:office|exam|quiz|deadline|due|assignment|week\s+\d|study\s+group)\b/i.test(line)) continue;
    const day = daysIn(line);
    const time = timeIn(line);
    // Only explicit schedule labels or a recurring weekday + time establish a meeting.
    const recurringLine = /^(?:mon(?:day)?s?|tue(?:s(?:day)?)?s?|wed(?:nesday)?s?|thu(?:rs(?:day)?)?s?|fri(?:day)?s?|sat(?:urday)?s?|sun(?:day)?s?|MWF|MW|MF|WF|TR|TTh|TuTh|MTWRF|[MTWRFSU])\b/i.test(line);
    if (!meetingLabel && !(recurringLine && day && time)) continue;
    const next = lines[i + 1];
    const value = meetingLabel && !day && !time && next && !/\b(?:office|exam|due|instructor|room|location)\b/i.test(next) ? next : line;
    const nextDays = day ?? daysIn(value);
    const nextTime = time ?? timeIn(value);
    const kind = /\b(lecture|lab)\b/i.exec(line)?.[1];
    const prefix = kind ? `${kind[0].toUpperCase()}${kind.slice(1).toLowerCase()}: ` : "";
    if (nextDays) days.push(prefix + nextDays);
    if (nextTime) times.push(prefix + nextTime);
    if (value !== line) i += 1;
  }
  return { days: days.length ? [...new Set(days)].join("; ") : null, time: times.length ? [...new Set(times)].join("; ") : null };
}
