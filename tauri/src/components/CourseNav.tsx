import { NavLink } from "react-router-dom";

// Persistent course-scoped sub-nav. Renders at the top of every page that
// lives under `/course/:id/...` so the user can jump between Grades,
// Assignments, Modules, Discussions, and the course Overview without
// going back to the dashboard each time.

interface Props {
  courseId: string;
  /** Optional: short label to display in the leading breadcrumb area. */
  courseLabel?: string;
}

const SECTIONS: { to: string; icon: string; label: string; end?: boolean }[] = [
  { to: "", icon: "fa-house", label: "Overview", end: true },
  { to: "/grades", icon: "fa-poll", label: "Grades" },
  { to: "/assignments", icon: "fa-tasks", label: "Assignments" },
  { to: "/content", icon: "fa-folder-tree", label: "Modules" },
  { to: "/discussions", icon: "fa-comments", label: "Discussions" },
];

export default function CourseNav({ courseId, courseLabel }: Props) {
  const base = `/course/${courseId}`;
  return (
    <div className="course-nav mb-4 is-flex is-align-items-center is-flex-wrap-wrap" style={{ gap: 6 }}>
      {courseLabel && (
        <span className="is-size-7 has-text-grey mr-2" style={{ fontWeight: 600 }}>
          {courseLabel}
        </span>
      )}
      {SECTIONS.map((s) => (
        <NavLink
          key={s.to}
          to={`${base}${s.to}`}
          end={s.end}
          className={({ isActive }) =>
            `button is-small ${isActive ? "is-primary" : "is-light"}`
          }
        >
          <span className="icon is-small"><i className={`fas ${s.icon}`}></i></span>
          <span>{s.label}</span>
        </NavLink>
      ))}
    </div>
  );
}
