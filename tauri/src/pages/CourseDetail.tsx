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

  return (
    <div>
      <nav className="breadcrumb mb-3">
        <ul>
          <li><Link to="/dashboard">Dashboard</Link></li>
          <li className="is-active"><a>{course.name}</a></li>
        </ul>
      </nav>
      <h1 className="title">{course.name}</h1>
      <div className="buttons">
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
