import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import { displayCourseName, type Course } from "../types";
import CourseNav from "./CourseNav";

// Shared course-page header: banner image + course title + CourseNav strip.
// Used by every page under `/course/:id/*` except CourseDetail (which has its
// own elaborate hero with color picker / sync controls).
//
// Title sits BELOW the banner in a clean white band rather than overlaid,
// because user banners vary wildly and a darkening overlay can't reliably keep
// the text legible against every photographic / graphic background. Banner has
// fixed height + object-cover so the layout doesn't shift between sub-pages.

interface Props {
  courseId: string;
}

const BANNER_HEIGHT = 140;

export default function CourseHeader({ courseId }: Props) {
  const [course, setCourse] = useState<Course | null>(null);

  useEffect(() => {
    api.getCourse(courseId).then(setCourse).catch(() => setCourse(null));
  }, [courseId]);

  const accent = course?.custom_color || "#739AC3";
  const banner = course?.banner_url;
  const title = course ? displayCourseName(course) : "";
  const code = course?.code;

  return (
    <header className="course-header mb-4" style={{ borderRadius: 12, overflow: "hidden", border: "1px solid rgba(0,0,0,0.08)", background: "white" }}>
      <div
        style={{
          height: BANNER_HEIGHT,
          backgroundImage: banner ? `url(${banner})` : undefined,
          backgroundSize: "cover",
          backgroundPosition: "center",
          backgroundColor: banner ? undefined : accent,
        }}
        aria-label={course ? `${title} banner` : "Course banner"}
      />
      <div style={{ padding: "0.75rem 1rem 0.5rem 1rem", borderTop: `3px solid ${accent}` }}>
        <nav className="breadcrumb is-small mb-1" aria-label="breadcrumbs">
          <ul>
            <li><Link to="/dashboard">Dashboard</Link></li>
            <li className="is-active"><a aria-current="page">{title || courseId}</a></li>
          </ul>
        </nav>
        <h1 className="title is-4 mb-2" style={{ color: accent }}>
          {code ? `${code} — ${title}` : title}
        </h1>
        <CourseNav courseId={courseId} />
      </div>
    </header>
  );
}
