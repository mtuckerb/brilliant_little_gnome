import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import type { Course } from "../types";

export default function Archive() {
  const [courses, setCourses] = useState<Course[] | null>(null);

  useEffect(() => {
    api.listCourses().then((c) => setCourses(c.filter((x) => x.status === "dropped" || x.status === "archived")));
  }, []);

  if (!courses) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  async function restore(id: string) {
    await api.dropCourse(id, "active");
    const c = await api.listCourses();
    setCourses(c.filter((x) => x.status === "dropped" || x.status === "archived"));
  }

  return (
    <div>
      <h1 className="title"><i className="fas fa-box-archive mr-2"></i>Archive</h1>
      <p className="subtitle is-6 has-text-grey">Dropped or archived courses. Sync data is kept locally.</p>
      <div className="box">
        {courses.length === 0 && <p className="has-text-grey py-4 has-text-centered">Nothing archived.</p>}
        {courses.map((c) => (
          <div key={c.org_unit_id} className="is-flex is-justify-content-space-between py-2" style={{ borderBottom: "1px solid #eee" }}>
            <div>
              <Link to={`/course/${c.org_unit_id}/grades`} style={{ color: c.custom_color || undefined }}>
                <strong>{c.code || c.name}</strong>
              </Link>
              <span className="ml-2 has-text-grey is-size-7">{c.semester} · {c.status}</span>
            </div>
            <button className="button is-small" onClick={() => restore(c.org_unit_id)}>Restore</button>
          </div>
        ))}
      </div>
    </div>
  );
}
