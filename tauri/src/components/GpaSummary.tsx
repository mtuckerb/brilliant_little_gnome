import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Link } from "react-router-dom";
import { api, onAppEvent } from "../api";
import type { DashboardGpa } from "../types";
import { fmtNum } from "../lib/format";

export default function GpaSummary() {
  const [summary, setSummary] = useState<DashboardGpa | null>(null);
  const [error, setError] = useState(false);
  const request = useRef(0);
  const refresh = useCallback(async () => {
    const current = ++request.current;
    try {
      const next = await api.dashboardGpa();
      if (current !== request.current) return;
      setSummary(next);
      setError(false);
    } catch {
      if (current === request.current) setError(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const reload = () => { void refresh(); };
    const subscriptions = [
      listen("grades:updated", reload),
      listen("course:updated", reload),
      listen("prefs:updated", reload),
      onAppEvent((event) => {
        if (event.kind === "grades_updated" ||
            (event.kind === "sync_status_changed" && event.status.status === "idle")) reload();
      }),
    ];
    return () => {
      request.current += 1;
      subscriptions.forEach((subscription) => { void subscription.then((unlisten) => unlisten()).catch(() => {}); });
    };
  }, [refresh]);

  const confidence = summary ? Math.round(summary.confidence * 100) : 0;
  return (
    <section className="box mb-4" aria-labelledby="overall-gpa-title">
      <h2 id="overall-gpa-title" className="title is-5 mb-4">Overall GPA</h2>
      {error && (
        <div className="notification is-warning is-light py-2 px-3 is-size-7" role="alert">
          {summary ? "GPA could not refresh. Showing the last loaded summary." : "GPA could not be loaded."}
          <button type="button" className="button is-small is-text ml-2" onClick={() => void refresh()}>Retry</button>
        </div>
      )}
      {!summary && !error && <p className="has-text-grey" role="status">Loading GPA…</p>}
      {summary && (
        <>
          <div className="columns is-mobile is-multiline mb-1">
            <div className="column is-half-mobile is-one-third-tablet">
              <p className="heading">Locked-in GPA</p>
              <p className="title is-3 mb-1" data-testid="locked-in-gpa">{summary.locked_in_gpa === null ? "—" : summary.locked_in_gpa.toFixed(2)}</p>
              <p className="is-size-7 has-text-grey">{summary.locked_in_gpa === null ? "No completed grades yet" : `${fmtNum(summary.locked_in_units)} completed credits`}</p>
              <p className="is-size-7 has-text-grey">Completed terms + historic grades</p>
            </div>
            <div className="column is-half-mobile is-one-third-tablet">
              <p className="heading">Projected GPA</p>
              <p className="title is-3 has-text-primary mb-1" data-testid="projected-gpa">{summary.projected_gpa === null ? "—" : summary.projected_gpa.toFixed(2)}</p>
              <p className="is-size-7 has-text-grey">{summary.projected_gpa === null ? "No GPA data yet" : `${fmtNum(summary.projected_units)} total credits`}</p>
              {summary.provisional_course_count > 0 && <span className="tag is-small is-info is-light mt-1">Provisional grades</span>}
            </div>
            <div className="column is-full-mobile is-one-third-tablet">
              <p className="heading">Projection confidence</p>
              <p className="title is-3 mb-2" data-testid="gpa-confidence">{summary.projected_gpa === null ? "—" : `${confidence}%`}</p>
              <progress className="progress is-small is-primary mb-1" value={confidence} max={100} aria-label="Official grade coverage">{confidence}%</progress>
              <p className="is-size-7 has-text-grey">Official-grade coverage</p>
            </div>
          </div>
          <p className="is-size-7 has-text-grey">
            Projection uses provisional grades and course targets for remaining work.
            Confidence measures official-grade coverage, not a probability.
          </p>
          {summary.excluded_course_count > 0 && <p className="is-size-7 has-text-grey mt-1">{summary.excluded_course_count} course{summary.excluded_course_count === 1 ? "" : "s"} without grade data excluded.</p>}
          {summary.projected_gpa === null && <Link className="is-size-7" to="/settings">Add historic GPA and credits in Settings</Link>}
        </>
      )}
    </section>
  );
}
