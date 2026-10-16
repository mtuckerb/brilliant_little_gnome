import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { Student } from "../types";
import { useToast } from "../components/ToastProvider";

const IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);

export default function Students() {
  const [students, setStudents] = useState<Student[]>([]);
  const [loading, setLoading] = useState(true);
  const [importing, setImporting] = useState(false);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const toast = useToast();

  useEffect(() => {
    let active = true;
    const load = async () => {
      const next = await api.listStudents();
      if (active) {
        setStudents(next);
        setError(null);
      }
    };
    // Show the saved directory immediately while previously unseen classes import.
    load().catch((e) => { if (active) setError(String(e)); })
      .finally(() => { if (active) setLoading(false); });
    setImporting(true);
    api.importStudents(false).then(async (result) => {
      await load();
      if (active && result.failed_courses.length > 0) {
        setNotice(`Could not import ${result.failed_courses.join(", ")}. Saved students are still available. Try Import all classes when access returns.`);
      }
    }).catch((e) => { if (active) setNotice(`Import unavailable: ${String(e)}. Saved students are still available.`); })
      .finally(() => { if (active) setImporting(false); });
    const unlisten = listen("students:updated", () => { load().catch(() => {}); });
    return () => {
      active = false;
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, []);

  async function importAll() {
    setImporting(true);
    setNotice(null);
    try {
      const result = await api.importStudents(true);
      setStudents(await api.listStudents());
      setError(null);
      if (result.failed_courses.length > 0) {
        setNotice(`Could not import ${result.failed_courses.join(", ")}. Their saved students have been kept.`);
      } else {
        toast.show(`Imported ${result.imported_courses} class lists.`, "is-success", 3500);
      }
    } catch (e) {
      setNotice(`Import unavailable: ${String(e)}. Your saved directory is still here.`);
    } finally {
      setImporting(false);
    }
  }

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return students.filter((student) => [student.display_name, student.email, student.pronouns,
      ...student.courses.flatMap((course) => [course.name, course.semester])]
      .some((value) => value?.toLowerCase().includes(needle)));
  }, [students, query]);

  return (
    <div>
      <div className="students-header">
        <div>
          <h1 className="title"><i className="fas fa-user-group mr-2" aria-hidden="true"></i>Students</h1>
          <p className="subtitle is-6 has-text-grey">Students from all your classes, past and present.</p>
        </div>
        <button className="button is-light" onClick={importAll} disabled={importing}>
          <span className="icon"><i className={`fas fa-sync ${importing ? "fa-spin" : ""}`} aria-hidden="true"></i></span>
          <span>{importing ? "Importing classes…" : "Import all classes"}</span>
        </button>
      </div>
      {error && <p role="alert" className="notification is-danger is-light">Could not load students: {error}</p>}
      {notice && <p role="status" className="notification is-warning is-light">{notice}</p>}
      <div className="box">
        <div className="field">
          <label className="label" htmlFor="student-search">Find a student</label>
          <input id="student-search" className="input" type="search" placeholder="Search by name, email, class, or semester"
            value={query} onChange={(event) => setQuery(event.target.value)} />
        </div>
        <p className="help mb-4" aria-live="polite">{visible.length} of {students.length} students · Photos and edits are saved on this device.</p>
        {loading ? <p role="status">Loading saved students…</p> : students.length === 0 ? (
          <p className="has-text-grey py-4">{importing ? "Importing students from your class lists…" : "No students saved yet. Import all classes to build your directory from the class lists you can access."}</p>
        ) : visible.length === 0 ? <p className="has-text-grey py-4">No students match “{query}”.</p> : (
          <div className="table-container">
            <table className="table is-fullwidth students-table">
              <thead><tr><th scope="col">Avatar</th><th scope="col">Student</th><th scope="col">Email</th><th scope="col">Classes together</th></tr></thead>
              <tbody>{visible.map((student) => <tr key={student.brightspace_user_id}>
                <td><StudentAvatar student={student} onSaved={(avatar) => {
                  setStudents((current) => current.map((person) => person.brightspace_user_id === student.brightspace_user_id
                    ? { ...person, avatar, avatar_overridden: true } : person));
                }} /></td>
                <th scope="row"><strong>{student.display_name}</strong>{student.pronouns && <p className="is-size-7 has-text-grey">{student.pronouns}</p>}</th>
                <td>{student.email ? <a href={`mailto:${student.email}`}>{student.email}</a> : <span className="has-text-grey">Unavailable</span>}</td>
                <td><ul className="students-courses">{student.courses.map((course) => <li key={course.course_id}>
                  {course.available ? <Link to={`/course/${course.course_id}`}>{course.name}</Link> : course.name}
                  {course.semester && <span className="has-text-grey is-size-7"> · {course.semester}</span>}
                </li>)}</ul></td>
              </tr>)}</tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}

function StudentAvatar({ student, onSaved }: { student: Student; onSaved: (avatar: string | null) => void }) {
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const initials = (student.first_name && student.last_name ? [student.first_name, student.last_name] : student.display_name.split(/\s+/))
    .slice(0, 2).map((part) => part.charAt(0).toUpperCase()).join("") || "?";

  async function save(avatar: string | null) {
    setSaving(true);
    setError(null);
    try {
      await api.updateStudentAvatar(student.brightspace_user_id, avatar);
      onSaved(avatar);
    } catch (e) {
      setError(`Photo could not be saved: ${String(e)}`);
    } finally {
      setSaving(false);
    }
  }

  async function upload(file: File | undefined) {
    if (!file) return;
    if (!IMAGE_TYPES.has(file.type) || file.size > 2 * 1024 * 1024) {
      setError("Choose a PNG, JPEG, WebP, or GIF smaller than 2 MB.");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const data = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(new Error("Could not read the image."));
        reader.readAsDataURL(file);
      });
      await save(data);
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  }

  return <div className="student-avatar-editor">
    <div className="student-avatar" aria-hidden="true">{student.avatar ? <img src={student.avatar} alt="" /> : initials}</div>
    <label className={`button is-small is-light student-photo-upload${saving ? " is-loading" : ""}`}>
      <span>{student.avatar ? "Change photo" : "Add photo"}</span>
      <input type="file" accept="image/png,image/jpeg,image/webp,image/gif" disabled={saving}
        aria-label={`Change photo for ${student.display_name}`}
        onChange={(event) => { const file = event.target.files?.[0]; event.target.value = ""; void upload(file); }} />
    </label>
    {student.avatar && <button className="button is-small is-text" disabled={saving} onClick={() => save(null)}
      aria-label={`Remove photo for ${student.display_name}`}>Remove</button>}
    {error && <p role="alert" className="help is-danger">{error}</p>}
  </div>;
}
