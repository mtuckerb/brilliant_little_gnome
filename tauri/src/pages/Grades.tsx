import { useEffect, useState, useCallback } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { GradeRow, GradeStats } from "../types";
import { fmtNum, fmtPct } from "../lib/format";
import { canSpeculateGrade, isActuallyGraded } from "../lib/grades";
import CourseHeader from "../components/CourseHeader";

function gradeColor(score: number | null): string {
  if (score === null) return "is-light";
  if (score >= 90) return "is-primary";
  if (score >= 80) return "is-warning";
  return score < 70 ? "is-danger" : "is-warning";
}
function confColor(c: number): string {
  if (c >= 70) return "has-text-primary";
  return c < 30 ? "has-text-danger" : "has-text-warning-dark";
}

export default function Grades() {
  const { id } = useParams<{ id: string }>();
  const [grades, setGrades] = useState<GradeRow[] | null>(null);
  const [stats, setStats] = useState<GradeStats | null>(null);
  const [showHidden, setShowHidden] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingValue, setEditingValue] = useState("");

  const load = useCallback(() => {
    if (!id) return;
    api.gradesSummary(id, showHidden).then((d) => {
      setGrades(d.rows);
      setStats(d.stats);
    });
  }, [id, showHidden]);

  useEffect(() => { load(); }, [load]);

  if (!grades || !stats) {
    return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;
  }

  // Backend returns confidence as a 0..1 fraction (possible / all_possible).
  // The display + color thresholds + "less than 100" notification gate all
  // assume a 0..100 percentage, so normalize once.
  const confidencePct = stats.confidence * 100;

  function startEdit(g: GradeRow) {
    setEditingId(g.id);
    setEditingValue(g.expected_score === null ? "" : String(g.expected_score));
  }

  function cancelEdit() {
    setEditingId(null);
    setEditingValue("");
  }

  async function commitEdit(g: GradeRow) {
    const trimmed = editingValue.trim();
    const num = trimmed === "" ? null : parseFloat(trimmed);
    // Empty clears; valid number sets; nonsense input cancels silently.
    if (num !== null && (Number.isNaN(num) || num < 0)) {
      cancelEdit();
      return;
    }
    setEditingId(null);
    setEditingValue("");
    await api.setExpectedScore(g.id, num);
    load();
  }

  function renderExpectedInput(g: GradeRow) {
    return (
      <input
        autoFocus
        type="number"
        step={0.1}
        min={0}
        value={editingValue}
        onChange={(e) => setEditingValue(e.target.value)}
        onBlur={() => commitEdit(g)}
        onKeyDown={(e) => {
          if (e.key === "Enter") commitEdit(g);
          if (e.key === "Escape") cancelEdit();
        }}
        placeholder="%"
        style={{
          width: 70,
          textAlign: "right",
          padding: "2px 6px",
          border: "1px solid #739AC3",
          borderRadius: 4,
          background: "#FFFFFF",
          fontSize: "0.85rem",
        }}
      />
    );
  }

  return (
    <div>
      <nav className="breadcrumb mb-3">
        <ul>
          <li><Link to="/dashboard">Dashboard</Link></li>
          <li><Link to={`/course/${id}`}>Course</Link></li>
          <li className="is-active"><a>Grades</a></li>
        </ul>
      </nav>
      {id && <CourseHeader courseId={id} />}

      <div className="box">
        <div className="level mb-5">
          <div className="level-left">
            <div>
              <h4 className="title is-4 mb-0"><i className="fas fa-poll mr-2"></i>Grades</h4>
              <p className="is-size-7 has-text-grey">Points-based grading (weight is proportional to points).</p>
            </div>
          </div>
          <div className="level-right">
            {stats.all_possible_points > 0 && (
              <>
                <div className="level-item has-text-centered px-4">
                  <div>
                    <p className="heading">Current Grade</p>
                    <p className={`title ${gradeColor(stats.score)}`}>{stats.score !== null ? fmtPct(stats.score) : "TBD"}</p>
                  </div>
                </div>
                <div className="level-item has-text-centered px-4" style={{ borderLeft: "1px solid #eee" }}>
                  <div>
                    <p className="heading">Confidence</p>
                    <p className={`title ${confColor(confidencePct)}`}>{fmtPct(confidencePct, 0)}</p>
                  </div>
                </div>
              </>
            )}
            <div className="level-item ml-4">
              <button className="button is-small" onClick={() => setShowHidden((s) => !s)}>
                <span className="icon"><i className={`fas ${showHidden ? "fa-eye-slash" : "fa-eye"}`}></i></span>
                <span>{showHidden ? "Hide Hidden" : "Show Hidden"}</span>
              </button>
            </div>
          </div>
        </div>

        {confidencePct < 100 && stats.remaining_points > 0 && (
          <div className="notification is-light is-info py-2 px-4 is-size-7 mb-4">
            <strong>Confidence Note:</strong> Only {fmtNum(stats.total_points_possible)} of the {fmtNum(stats.all_possible_points)} total points graded.
          </div>
        )}

        <table className="table is-fullwidth is-hoverable">
          <thead>
            <tr>
              <th>Item</th>
              <th className="has-text-centered">Due Date</th>
              <th className="has-text-centered">Relative Weight</th>
              <th className="has-text-centered">Points</th>
              <th className="has-text-right">Result</th>
            </tr>
          </thead>
          <tbody>
            {grades.map((g) => {
              const actuallyGraded = isActuallyGraded(g);
              const rowClass = g.hidden ? "has-background-white-ter has-text-grey-light" : (actuallyGraded ? "" : "has-background-white-ter has-text-grey-light");
              let result;
              if (editingId === g.id) {
                result = renderExpectedInput(g);
              } else if (actuallyGraded && g.perc !== null) {
                result = <span className={`tag ${gradeColor(g.perc)} is-light`} style={{ fontWeight: "bold" }}>{fmtPct(g.perc)}</span>;
              } else if (g.is_expected && g.expected_score !== null) {
                result = (
                  <a
                    onClick={() => startEdit(g)}
                    title="Expected score (click to edit)"
                    style={{ fontStyle: "italic", color: "#888", textDecoration: "none", borderBottom: "1px dotted #bbb", cursor: "text" }}
                  >
                    ~{fmtPct(g.expected_score)}
                  </a>
                );
              } else if (canSpeculateGrade(g)) {
                result = (
                  <a
                    onClick={() => startEdit(g)}
                    title="Click to speculate a score"
                    className="has-text-grey-light"
                    style={{ cursor: "text", borderBottom: "1px dotted #ddd" }}
                  >
                    —
                  </a>
                );
              } else {
                result = <>—</>;
              }

              const points = actuallyGraded ? `${fmtNum(g.numerator)} / ${fmtNum(g.denominator, 2, "-")}` :
                g.is_expected ? `${fmtNum(g.numerator ?? 0)} / ${fmtNum(g.denominator ?? g.rel_weight, 2, "-")}` :
                `- / ${fmtNum(g.denominator, 2, "-")}`;

              return (
                <tr key={g.id} className={rowClass} style={{ opacity: g.hidden ? 0.55 : 1 }}>
                  <td>
                    {g.name}
                    {g.hidden && <span className="tag is-small is-dark is-light ml-2"><i className="fas fa-eye-slash mr-1"></i>Hidden</span>}
                    {g.submitted === true && <span className="tag is-small is-success is-light ml-2"><i className="fas fa-check mr-1"></i>Submitted</span>}
                    {g.submitted === false && <span className="tag is-small is-danger is-light ml-2"><i className="fas fa-exclamation-circle mr-1"></i>Not Submitted</span>}
                    {g.manually_marked_ungraded && <span className="tag is-small is-warning is-light ml-2">Manually Ungraded</span>}
                    {!g.is_graded && !g.manually_marked_ungraded && <span className="tag is-small is-light ml-2">Ungraded</span>}
                    {g.is_extra_credit && <span className="tag is-small is-warning ml-2">Extra Credit</span>}
                    <div className="is-pulled-right">
                      <button className="button is-ghost is-small p-0 mr-2" onClick={() => api.toggleGradeHidden(g.id).then(load)} title="Hide/unhide">
                        <i className={`fas ${g.hidden ? "fa-eye" : "fa-trash-alt"} ${g.hidden ? "has-text-grey" : "has-text-grey-lighter"}`}></i>
                      </button>
                      <button className="button is-ghost is-small p-0 mr-2" onClick={() => api.toggleGradeUngraded(g.id).then(load)} title="Toggle ungraded">
                        <i className={`fas fa-eye-slash ${g.manually_marked_ungraded ? "has-text-warning" : "has-text-grey-lighter"}`}></i>
                      </button>
                      <button className="button is-ghost is-small p-0" onClick={() => api.toggleGradeExtraCredit(g.id).then(load)} title="Toggle extra credit">
                        <i className={`fas fa-star ${g.is_extra_credit ? "has-text-warning" : "has-text-grey-lighter"}`}></i>
                      </button>
                    </div>
                  </td>
                  <td className="has-text-centered is-size-7">
                    {g.due_date ? new Date(g.due_date).toLocaleDateString([], { month: "short", day: "numeric" }) : "-"}
                  </td>
                  <td className="has-text-centered is-size-7">{fmtPct(g.rel_weight)}</td>
                  <td className="has-text-centered is-size-7" style={{ fontFamily: "monospace" }}>{points}</td>
                  <td className="has-text-right">{result}</td>
                </tr>
              );
            })}
          </tbody>
          {stats.all_possible_points > 0 && (
            <tfoot>
              <tr className="has-background-white-bis">
                <td className="has-text-weight-bold">Course Total</td>
                <td></td>
                <td className="has-text-centered has-text-weight-bold">100%</td>
                <td className="has-text-centered">
                  <span className="is-size-7 has-text-weight-bold">{fmtNum(stats.total_points_earned)} / {fmtNum(stats.total_points_possible)} pts</span>
                </td>
                <td className="has-text-right">
                  <span className={`tag ${gradeColor(stats.score)} is-medium`} style={{ fontWeight: "bold" }}>
                    {stats.score !== null ? fmtPct(stats.score) : "TBD"}
                  </span>
                </td>
              </tr>
            </tfoot>
          )}
        </table>
      </div>
    </div>
  );
}
