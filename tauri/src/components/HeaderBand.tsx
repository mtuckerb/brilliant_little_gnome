import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import {
  displayCourseCode,
  displayCourseName,
  stripLeadingCode,
  type Course,
} from "../types";
import CourseNav from "./CourseNav";
import BrightspaceLink, { useBrightspaceHost } from "./BrightspaceLink";
import { courseHomeUrl } from "../lib/brightspace";
import { useIsMobile } from "../hooks/useIsMobile";

// Shared course-page header: banner image + breadcrumb + code/title (both
// click-to-edit) + CourseNav strip. Used by every page under `/course/:id/*`,
// including CourseDetail. Banner sits above a clean info band so user banners
// don't fight with readability — see prior comment history for rationale.

interface Props {
  courseId: string;
  // Optional callback for parents that keep their own copy of the course
  // (CourseDetail uses this so its settings panels reflect title/code edits).
  onCourseUpdated?: (course: Course) => void;
}

const BANNER_HEIGHT_DESKTOP = 140;
const BANNER_HEIGHT_MOBILE = 84;

// Brightspace banner URLs require an authenticated session cookie that the
// webview can't carry. We proxy through Rust and turn the bytes into a data
// URL — cached per courseId for the lifetime of the page so navigation
// between sub-pages doesn't re-fetch.
const bannerCache = new Map<string, string | null>();

export default function HeaderBand({ courseId, onCourseUpdated }: Props) {
  const [course, setCourse] = useState<Course | null>(null);
  const [bannerDataUrl, setBannerDataUrl] = useState<string | null>(
    () => bannerCache.get(courseId) ?? null,
  );
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState("");
  const [editingCode, setEditingCode] = useState(false);
  const [codeDraft, setCodeDraft] = useState("");
  const titleInputRef = useRef<HTMLInputElement>(null);
  const codeInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api.getCourse(courseId).then(setCourse).catch(() => setCourse(null));
  }, [courseId]);

  useEffect(() => {
    if (bannerCache.has(courseId)) {
      setBannerDataUrl(bannerCache.get(courseId) ?? null);
      return;
    }
    let cancelled = false;
    api
      .fetchCourseBanner(courseId)
      .then((res) => {
        const dataUrl = res?.data_url ?? null;
        bannerCache.set(courseId, dataUrl);
        if (!cancelled) setBannerDataUrl(dataUrl);
      })
      .catch(() => {
        bannerCache.set(courseId, null);
        if (!cancelled) setBannerDataUrl(null);
      });
    return () => { cancelled = true; };
  }, [courseId]);

  const accent = course?.custom_color || "#739AC3";
  const banner = bannerDataUrl;
  const bsHost = useBrightspaceHost();
  const isMobile = useIsMobile();
  const bannerHeight = isMobile ? BANNER_HEIGHT_MOBILE : BANNER_HEIGHT_DESKTOP;
  const cleanTitle = course ? stripLeadingCode(displayCourseName(course)) : "";
  const code = course ? displayCourseCode(course) : null;
  const breadcrumbLabel = course ? displayCourseName(course) : courseId;

  function startTitleEdit() {
    if (!course) return;
    setTitleDraft(cleanTitle);
    setEditingTitle(true);
    setTimeout(() => titleInputRef.current?.focus(), 0);
  }

  function startCodeEdit() {
    if (!course) return;
    setCodeDraft(code ?? "");
    setEditingCode(true);
    setTimeout(() => codeInputRef.current?.focus(), 0);
  }

  async function saveTitle() {
    if (!course) return;
    setEditingTitle(false);
    const draft = titleDraft.trim();
    if (draft === (course.custom_name ?? "").trim() && draft !== "") return;
    try {
      const updated = await api.updateCourseName(course.org_unit_id, draft);
      setCourse(updated);
      onCourseUpdated?.(updated);
    } catch (e) {
      alert(`Title save failed: ${String((e as { message?: string })?.message ?? e)}`);
    }
  }

  async function saveCode() {
    if (!course) return;
    setEditingCode(false);
    const draft = codeDraft.trim();
    const next = draft === "" ? null : draft;
    if ((course.custom_code ?? null) === next) return;
    const previous = course;
    const optimistic: Course = { ...course, custom_code: next };
    setCourse(optimistic);
    onCourseUpdated?.(optimistic);
    try {
      await api.updateCourseCode(course.org_unit_id, next);
    } catch (e) {
      setCourse(previous);
      onCourseUpdated?.(previous);
      alert(`Code save failed: ${String((e as { message?: string })?.message ?? e)}`);
    }
  }

  return (
    <header
      className="course-header mb-4"
      style={{
        borderRadius: 12,
        overflow: "hidden",
        border: "1px solid rgba(0,0,0,0.08)",
        background: "white",
      }}
    >
      <div
        style={{
          height: bannerHeight,
          backgroundImage: banner ? `url(${banner})` : undefined,
          backgroundSize: "cover",
          backgroundPosition: "center",
          backgroundColor: banner ? undefined : accent,
        }}
        aria-label={course ? `${displayCourseName(course)} banner` : "Course banner"}
      />
      <div style={{ padding: "0.75rem 1rem 0.5rem 1rem", borderTop: `3px solid ${accent}` }}>
        <nav className="breadcrumb is-small mb-1" aria-label="breadcrumbs">
          <ul>
            <li><Link to="/dashboard">Dashboard</Link></li>
            <li className="is-active"><a aria-current="page">{breadcrumbLabel}</a></li>
          </ul>
        </nav>
        <div
          className="is-flex is-align-items-baseline is-flex-wrap-wrap mb-2"
          style={{ gap: 10 }}
        >
          {editingCode ? (
            <input
              ref={codeInputRef}
              className="input is-small"
              value={codeDraft}
              onChange={(e) => setCodeDraft(e.target.value)}
              onBlur={saveCode}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveCode();
                if (e.key === "Escape") setEditingCode(false);
              }}
              placeholder="code"
              style={{ width: "12ch", fontWeight: 600, letterSpacing: 1 }}
            />
          ) : (
            <button
              type="button"
              onClick={startCodeEdit}
              title={course ? "Click to edit course code" : undefined}
              className="tag is-medium"
              style={{
                backgroundColor: accent,
                color: "white",
                fontWeight: 700,
                letterSpacing: 1,
                cursor: course ? "text" : "default",
                border: "none",
              }}
            >
              {code || "set code"}
            </button>
          )}
          {editingTitle ? (
            <input
              ref={titleInputRef}
              className="input"
              value={titleDraft}
              onChange={(e) => setTitleDraft(e.target.value)}
              onBlur={saveTitle}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveTitle();
                if (e.key === "Escape") setEditingTitle(false);
              }}
              placeholder="Course title"
              style={{ flex: "1 1 auto", fontSize: "1.25rem", fontWeight: 600 }}
            />
          ) : (
            <h1
              className="title is-4 mb-0"
              style={{
                color: accent,
                cursor: course ? "text" : "default",
                flex: "1 1 auto",
              }}
              title={course ? "Click to edit course title" : undefined}
              onClick={startTitleEdit}
            >
              {cleanTitle || (course ? "Untitled course" : "")}
            </h1>
          )}
          {bsHost && course && (
            <BrightspaceLink url={courseHomeUrl(bsHost, course.org_unit_id)} label="Open this course in Brightspace" />
          )}
        </div>
        <CourseNav courseId={courseId} />
      </div>
    </header>
  );
}
