import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { Course } from "../types";

export default function CourseDetail() {
  const { id } = useParams<{ id: string }>();
  const [course, setCourse] = useState<Course | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    api.getCourse(id).then(setCourse).catch((e) => setErr(String(e?.message ?? e)));
  }, [id]);

  if (err) return <div className="notification is-danger">{err}</div>;
  if (!course) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  // Local color edits commit on `change` (color pickers don't fire on every drag
  // step); we optimistically reflect the change so the picker doesn't lag.
  async function onColorChange(next: string) {
    if (!course) return;
    setCourse({ ...course, custom_color: next });
    await api.updateCourseColor(course.org_unit_id, next).catch(() => {});
  }

  const accent = course.custom_color || "#739AC3";
  const banner = course.banner_url;

  return (
    <div>
      <nav className="breadcrumb mb-3">
        <ul>
          <li><Link to="/dashboard">Dashboard</Link></li>
          <li className="is-active"><a>{course.name}</a></li>
        </ul>
      </nav>

      {banner ? (
        <section
          className="hero is-small mb-5"
          style={{
            borderRadius: 12,
            overflow: "hidden",
            position: "relative",
            backgroundImage: `url(${banner})`,
            backgroundSize: "cover",
            backgroundPosition: "center",
            border: `1px solid ${accent}`,
            boxShadow: "0 4px 15px rgba(0,0,0,0.1)",
          }}
        >
          <div
            className="hero-body"
            style={{
              background:
                "linear-gradient(to right, rgba(0,0,0,0.8) 0%, rgba(0,0,0,0.3) 50%, rgba(0,0,0,0.1) 100%)",
              padding: "3rem 2rem",
            }}
          >
            <h1
              className="title is-2 has-text-white"
              style={{ textShadow: "0 2px 10px rgba(0,0,0,0.8)", lineHeight: 1.1 }}
            >
              {course.name}
            </h1>
            {course.code && (
              <p
                className="is-size-7 has-text-white is-uppercase"
                style={{
                  letterSpacing: 1.5,
                  textShadow: "0 1px 2px rgba(0,0,0,0.5)",
                  position: "absolute",
                  bottom: 10,
                  right: 20,
                  opacity: 0.6,
                }}
              >
                {course.code}
              </p>
            )}
          </div>
        </section>
      ) : (
        <h1 className="title mb-4" style={{ color: accent }}>
          {course.code && <span className="mr-3">{course.code}</span>}
          {course.name}
        </h1>
      )}

      <div className="is-flex is-align-items-center mb-4" style={{ gap: 12 }}>
        <label className="is-flex is-align-items-center" style={{ gap: 8 }}>
          <span className="is-size-7 has-text-grey">Color</span>
          <input
            type="color"
            value={course.custom_color || "#739AC3"}
            onChange={(e) => onColorChange(e.target.value)}
            style={{ width: 36, height: 28, padding: 0, border: "1px solid #ccc", borderRadius: 4 }}
            title="Course color"
          />
        </label>
        <Link to={`/course/${course.org_unit_id}/grades`} className="button is-primary is-light">
          <span className="icon"><i className="fas fa-poll"></i></span><span>Grades</span>
        </Link>
        <Link to={`/course/${course.org_unit_id}/assignments`} className="button is-primary is-light">
          <span className="icon"><i className="fas fa-tasks"></i></span><span>Assignments</span>
        </Link>
        <button className="button is-small is-light" onClick={() => api.refreshCourse(course.org_unit_id)}>
          <span className="icon"><i className="fas fa-sync"></i></span><span>Refresh</span>
        </button>
      </div>
    </div>
  );
}
