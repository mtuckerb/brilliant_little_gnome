import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api } from "../api";
import {
  displayCourseName,
  type Assignment,
  type AssignmentAttachment,
  type AssignmentDetailPayload,
  type Course,
} from "../types";
import { fmtNum } from "../lib/format";
import CourseHeader from "../components/CourseHeader";

function isQuizAssignment(a: Assignment | null) {
  return a?.assignment_type?.toLowerCase() === "quiz";
}

function quizIdFromBrightspaceId(brightspaceId: string) {
  return brightspaceId.startsWith("quiz_") ? brightspaceId.slice("quiz_".length) : brightspaceId;
}

function brightspacePathForAssignment(a: Assignment, courseId: string) {
  if (isQuizAssignment(a)) {
    return `/d2l/lms/quizzing/user/quiz_summary.d2l?qi=${encodeURIComponent(quizIdFromBrightspaceId(a.brightspace_id))}&ou=${encodeURIComponent(courseId)}`;
  }
  return `/d2l/le/${encodeURIComponent(courseId)}/dropbox/${encodeURIComponent(a.brightspace_id)}`;
}

function errorMessage(e: unknown) {
  return String((e as { message?: string })?.message ?? e);
}

function AttachmentList({ items }: { items: AssignmentAttachment[] }) {
  if (items.length === 0) return null;
  return (
    <ul className="mt-2">
      {items.map((f, idx) => (
        <li key={idx} className="is-size-7">
          <span className="icon is-small mr-1"><i className="fas fa-paperclip"></i></span>
          {f.url ? (
            <a href={f.url} target="_blank" rel="noreferrer">{f.name}</a>
          ) : (
            <span>{f.name}</span>
          )}
          {f.size != null && <span className="has-text-grey ml-2">({Math.round(f.size / 1024)} KB)</span>}
        </li>
      ))}
    </ul>
  );
}

export default function AssignmentDetail() {
  const { id, aid } = useParams<{ id: string; aid: string }>();
  const navigate = useNavigate();
  const [course, setCourse] = useState<Course | null>(null);
  const [a, setA] = useState<Assignment | null>(null);
  const [detail, setDetail] = useState<AssignmentDetailPayload | null>(null);
  const [brightspaceHost, setBrightspaceHost] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [brightspaceOpenErr, setBrightspaceOpenErr] = useState<string | null>(null);

  const load = useCallback(() => {
    if (!id || !aid) return;
    const aidNum = Number(aid);
    setErr(null);
    setBrightspaceOpenErr(null);
    setA(null);
    setDetail(null);
    Promise.all([
      api.getCourse(id).catch(() => null),
      api.getPrefs().catch(() => null),
      api.listAssignments(id, undefined),
    ])
      .then(([c, prefs, list]) => {
        setCourse(c);
        setBrightspaceHost(prefs?.brightspace_host ?? null);
        const found = list.find((x) => x.id === aidNum) ?? null;
        setA(found);
        if (!found) {
          setErr("Assignment not found");
          return;
        }
        if (isQuizAssignment(found)) {
          return;
        }
        api
          .getAssignmentDetail(id, found.brightspace_id)
          .then(setDetail)
          .catch((e) => setErr(errorMessage(e)));
      })
      .catch((e) => setErr(errorMessage(e)));
  }, [id, aid]);

  useEffect(() => { load(); }, [load]);

  if (err) {
    return (
      <div>
        <nav className="breadcrumb mb-3">
          <ul>
            <li><Link to="/dashboard">Dashboard</Link></li>
            {id && <li><Link to={`/course/${id}`}>Course</Link></li>}
            {id && <li><Link to={`/course/${id}/assignments`}>Assignments</Link></li>}
            <li className="is-active"><a>Assignment unavailable</a></li>
          </ul>
        </nav>
        {id && <CourseHeader courseId={id} />}
        <div className="notification is-warning is-light">
          <p className="has-text-weight-semibold">We couldn't load this assignment.</p>
          <p className="is-size-7 mt-1">{err}</p>
          <div className="buttons mt-3">
            <button className="button is-small is-primary" onClick={load}>Retry</button>
            {id && <Link className="button is-small" to={`/course/${id}/assignments`}>Back to assignments</Link>}
            {id && <Link className="button is-small is-light" to={`/course/${id}`}>Back to course</Link>}
          </div>
        </div>
      </div>
    );
  }
  if (!a) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  async function toggleComplete() {
    if (!a) return;
    await api.toggleAssignmentComplete(a.id);
    setA({ ...a, completed: !a.completed });
  }
  async function toggleOptional() {
    if (!a) return;
    await api.toggleAssignmentOptional(a.id);
    setA({ ...a, optional: !a.optional });
  }

  const accent = course?.custom_color || "#739AC3";
  const isQuiz = isQuizAssignment(a);
  const brightspacePath = id ? brightspacePathForAssignment(a, id) : null;
  const brightspaceUrl = a.external_url || (brightspaceHost && brightspacePath ? `https://${brightspaceHost}${brightspacePath}` : null);

  function openBrightspace() {
    if (!brightspaceUrl) {
      setBrightspaceOpenErr("Brightspace login details are not available on this device.");
      return;
    }
    setBrightspaceOpenErr(null);
    try {
      window.open(brightspaceUrl, "_blank", "noopener,noreferrer");
    } catch (e) {
      setBrightspaceOpenErr(errorMessage(e));
    }
  }

  // Score cascade matches original Sinatra: feedback.Score → gradebook.numerator
  const fb = detail?.feedback;
  const gb = detail?.gradebook;
  const showScore = fb?.score_numerator != null || fb?.displayed_score || gb;
  const description = detail?.instructions_html ?? a.description;

  return (
    <div>
      <nav className="breadcrumb mb-3">
        <ul>
          <li><Link to="/dashboard">Dashboard</Link></li>
          {course && <li><Link to={`/course/${course.org_unit_id}`}>{course.code || displayCourseName(course)}</Link></li>}
          <li><Link to={`/course/${id}/assignments`}>Assignments</Link></li>
          <li className="is-active"><a>{a.name}</a></li>
        </ul>
      </nav>
      {id && <CourseHeader courseId={id} />}

      <div className="box">
        <h1 className="title is-4" style={{ color: accent }}>{a.name}</h1>

        <div className="tags mb-4">
          {a.completed && <span className="tag is-success">Done</span>}
          {a.optional && <span className="tag is-light">Optional</span>}
          {a.synthetic && <span className="tag is-info is-light">Synthetic</span>}
          {a.assignment_type && <span className="tag">{a.assignment_type}</span>}
          {a.is_graded && <span className="tag is-warning is-light">Graded</span>}
        </div>

        <div className="content is-size-6">
          <p>
            <strong>Due:</strong>{" "}
            {a.due_date
              ? new Date(a.due_date).toLocaleString(undefined, { dateStyle: "full", timeStyle: "short" })
              : <span className="has-text-grey">no due date</span>}
          </p>
          {a.completed_at && (
            <p><strong>Completed:</strong> {new Date(a.completed_at).toLocaleString()}</p>
          )}
        </div>

        <div className="buttons mt-3">
          <button className="button is-primary" onClick={toggleComplete}>
            <span className="icon"><i className={`fas ${a.completed ? "fa-undo" : "fa-check"}`}></i></span>
            <span>{a.completed ? "Mark incomplete" : "Mark complete"}</span>
          </button>
          <button className="button" onClick={toggleOptional}>
            <span>{a.optional ? "Required" : "Optional"}</span>
          </button>
          {(a.external_url || isQuiz) && (
            <button className="button is-link is-light" onClick={openBrightspace}>
              <span className="icon"><i className="fas fa-external-link-alt"></i></span>
              <span>{isQuiz ? "Open quiz" : "Open in Brightspace"}</span>
            </button>
          )}
          {a.synthetic && (
            <button
              className="button is-danger is-light"
              onClick={async () => {
                if (!a) return;
                if (!confirm(`Delete "${a.name}"? This can't be undone.`)) return;
                await api.deleteAssignment(a.id);
                navigate(`/course/${id}/assignments`);
              }}
            >
              <span className="icon"><i className="fas fa-trash"></i></span>
              <span>Delete</span>
            </button>
          )}
        </div>
      </div>

      {brightspaceOpenErr && (
        <div className="notification is-warning is-light">
          <p className="has-text-weight-semibold">We couldn't open Brightspace automatically.</p>
          <p className="is-size-7 mt-1">{brightspaceOpenErr}</p>
          <div className="buttons mt-3">
            {id && <Link className="button is-small" to={`/course/${id}/assignments`}>Back to assignments</Link>}
            {id && <Link className="button is-small is-light" to={`/course/${id}`}>Back to course</Link>}
          </div>
        </div>
      )}

      {isQuiz && (
        <div className="notification is-info is-light">
          <p className="has-text-weight-semibold">This assignment is a Brightspace quiz.</p>
          <p className="is-size-7 mt-1">Use Open quiz to launch it in Brightspace. You can always return here with the navigation links below.</p>
          <div className="buttons mt-3">
            <button className="button is-small is-link" onClick={openBrightspace}>Open quiz</button>
            {id && <Link className="button is-small" to={`/course/${id}/assignments`}>Back to assignments</Link>}
            {id && <Link className="button is-small is-light" to={`/course/${id}`}>Back to course</Link>}
          </div>
        </div>
      )}

      {detail === null && !a.synthetic && !isQuiz && !err && (
        <div className="has-text-centered py-3">
          <span className="icon has-text-primary"><i className="fas fa-circle-notch fa-spin"></i></span>
          <span className="ml-2 has-text-grey">Loading rubric, feedback &amp; submissions…</span>
        </div>
      )}

      {/* Score */}
      {showScore && (
        <div className="box">
          <h2 className="title is-5"><i className="fas fa-chart-bar mr-2"></i>Score</h2>
          {fb?.score_numerator != null && fb.score_denominator != null && (
            <p className="is-size-4">
              <strong>{fmtNum(fb.score_numerator)}</strong>
              <span className="has-text-grey"> / {fmtNum(fb.score_denominator)}</span>
            </p>
          )}
          {fb?.displayed_score && <p className="has-text-grey">{fb.displayed_score}</p>}
          {!fb?.score_numerator && gb && (
            <>
              {gb.displayed_grade && <p className="is-size-5">{gb.displayed_grade}</p>}
              {gb.numerator != null && gb.denominator != null && (
                <p className="is-size-6 has-text-grey">
                  {fmtNum(gb.numerator)} / {fmtNum(gb.denominator)}
                </p>
              )}
              <p className="is-size-7 has-text-grey-light"><em>from gradebook</em></p>
            </>
          )}
        </div>
      )}

      {/* Feedback */}
      {fb && (fb.feedback_html || fb.attachments.length > 0) && (
        <div className="box">
          <h2 className="title is-5"><i className="fas fa-comment-dots mr-2"></i>Feedback</h2>
          {fb.feedback_html && <div className="content" dangerouslySetInnerHTML={{ __html: fb.feedback_html }} />}
          <AttachmentList items={fb.attachments} />
        </div>
      )}

      {/* Gradebook comments fallback (only shown when no official feedback comment exists) */}
      {!fb?.feedback_html && gb?.comments_html && (
        <div className="box">
          <h2 className="title is-5"><i className="fas fa-comment-dots mr-2"></i>Gradebook comments</h2>
          <div className="content" dangerouslySetInnerHTML={{ __html: gb.comments_html }} />
        </div>
      )}

      {/* Instructions */}
      {description && (
        <div className="box">
          <h2 className="title is-5"><i className="fas fa-list mr-2"></i>Instructions</h2>
          <div className="content" dangerouslySetInnerHTML={{ __html: description }} />
          {detail && <AttachmentList items={detail.instruction_attachments} />}
        </div>
      )}

      {/* Submissions */}
      {detail && detail.submissions.length > 0 && (
        <div className="box">
          <h2 className="title is-5"><i className="fas fa-upload mr-2"></i>Your submissions</h2>
          {detail.submissions.map((s, idx) => (
            <div key={idx} className="mb-3 pb-2" style={{ borderBottom: "1px solid #eee" }}>
              <p className="is-size-7 has-text-grey">
                {s.submitted_at ? new Date(s.submitted_at).toLocaleString() : "(no date)"}
              </p>
              {s.comment_html && (
                <div className="content is-small" dangerouslySetInnerHTML={{ __html: s.comment_html }} />
              )}
              <AttachmentList items={s.files} />
            </div>
          ))}
        </div>
      )}

      {/* Rubric — present the raw payload as JSON for now; the legacy view's
          two-format (Assessments / CriteriaGroups) renderer can replace this
          when needed. */}
      {detail?.rubrics_raw != null && (
        <details className="box">
          <summary className="is-clickable"><strong><i className="fas fa-th mr-2"></i>Rubric</strong></summary>
          <pre className="is-size-7" style={{ whiteSpace: "pre-wrap", maxHeight: 320, overflow: "auto" }}>
            {JSON.stringify(detail.rubrics_raw, null, 2)}
          </pre>
        </details>
      )}
    </div>
  );
}
