import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { CourseRoster, CourseRosterPerson } from "../types";
import { classlistUrl } from "../lib/brightspace";
import { useBrightspaceHost } from "./BrightspaceLink";
import { useToast } from "./ToastProvider";
import { triggerDownload } from "../lib/download";

interface Props {
  courseId: string;
}

function isLearner(person: CourseRosterPerson): boolean {
  return /learner|student/i.test(person.role_name ?? "");
}

function initials(person: CourseRosterPerson): string {
  const parts = [person.first_name, person.last_name].filter(Boolean) as string[];
  const source = parts.length > 0 ? parts : person.display_name.split(/\s+/);
  return source.slice(0, 2).map((part) => part.charAt(0).toUpperCase()).join("") || "?";
}

function formatSyncedAt(value: string | null): string | null {
  if (!value) return null;
  const normalized = value.includes("T") ? value : `${value.replace(" ", "T")}Z`;
  const date = new Date(normalized);
  if (Number.isNaN(date.getTime())) return null;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

export default function ClassListPanel({ courseId }: Props) {
  const [roster, setRoster] = useState<CourseRoster | null>(null);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const host = useBrightspaceHost();
  const toast = useToast();

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setRoster(null);
    setQuery("");
    setError(null);
    api
      .getCourseRoster(courseId)
      .then((next) => { if (!cancelled) setRoster(next); })
      .catch((e) => { if (!cancelled) setError(String(e?.message ?? e)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [courseId]);

  useEffect(() => {
    let active = true;
    const unlisten = listen<{ course_id: string }>("course:updated", (event) => {
      if (!active || event.payload.course_id !== courseId) return;
      api.getCourseRoster(courseId).then((next) => {
        if (active) setRoster(next);
      }).catch(() => {});
    });
    return () => {
      active = false;
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, [courseId]);

  async function refresh() {
    if (refreshing) return;
    setRefreshing(true);
    setError(null);
    try {
      const next = await api.refreshCourseRoster(courseId);
      setRoster(next);
      toast.show(
        `Saved ${next.people.length} class-list entr${next.people.length === 1 ? "y" : "ies"}.`,
        "is-success",
        4000,
      );
    } catch (e) {
      const message = String((e as { message?: string })?.message ?? e);
      setError(message);
      toast.show(`Class list refresh failed: ${message}`, "is-danger", 7000);
    } finally {
      setRefreshing(false);
    }
  }

  async function openClassList() {
    if (!host) return;
    try {
      await api.openUrl(classlistUrl(host, courseId));
    } catch (e) {
      toast.show(
        `Could not open the Brightspace class list: ${String((e as { message?: string })?.message ?? e)}`,
        "is-danger",
        6000,
      );
    }
  }

  async function downloadCsv() {
    if (downloading) return;
    setDownloading(true);
    try {
      const payload = await api.downloadCourseRoster(courseId);
      triggerDownload(payload);
      toast.show(`Saved ${payload.filename}.`, "is-success", 5000);
    } catch (e) {
      toast.show(
        `Class list download failed: ${String((e as { message?: string })?.message ?? e)}`,
        "is-danger",
        6000,
      );
    } finally {
      setDownloading(false);
    }
  }

  async function copyEmail(email: string) {
    try {
      await navigator.clipboard.writeText(email);
      toast.show("Email address copied.", "is-success", 2500);
    } catch {
      toast.show("Could not copy the email address.", "is-danger", 3500);
    }
  }

  const people = roster?.people ?? [];
  const classmateCount = people.filter((person) => isLearner(person) && !person.is_current_user).length;
  const instructorCount = people.filter((person) => /instructor|teacher|professor/i.test(person.role_name ?? "") && !person.is_current_user).length;
  const visiblePeople = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return people;
    return people.filter((person) =>
      [person.display_name, person.email, person.role_name, person.pronouns]
        .filter(Boolean)
        .some((value) => value!.toLowerCase().includes(needle)),
    );
  }, [people, query]);
  const syncedAt = formatSyncedAt(roster?.synced_at ?? null);

  return (
    <section className="box class-list-panel" aria-labelledby={`class-list-title-${courseId}`}>
      <div className="class-list-header">
        <div>
          <h2 id={`class-list-title-${courseId}`} className="title is-6 mb-1">
            <i className="fas fa-user-group mr-2 has-text-primary"></i>Class list
          </h2>
          <p className="is-size-7 has-text-grey">
            {loading
              ? "Loading your saved class list…"
              : `${classmateCount} classmate${classmateCount === 1 ? "" : "s"}` +
                (instructorCount > 0 ? ` · ${instructorCount} instructor${instructorCount === 1 ? "" : "s"}` : "") +
                (syncedAt ? ` · Updated ${syncedAt}` : "")}
          </p>
        </div>
        <div className="buttons are-small mb-0">
          <button className="button is-light" onClick={refresh} disabled={refreshing || loading}>
            <span className="icon"><i className={`fas fa-sync ${refreshing ? "fa-spin" : ""}`}></i></span>
            <span>{refreshing ? "Refreshing…" : "Refresh"}</span>
          </button>
          <button className="button is-light" onClick={downloadCsv} disabled={downloading || loading || people.length === 0}>
            <span className="icon"><i className={`fas ${downloading ? "fa-circle-notch fa-spin" : "fa-file-csv"}`}></i></span>
            <span>{downloading ? "Saving…" : "Download CSV"}</span>
          </button>
          <button
            className="button is-light"
            onClick={openClassList}
            disabled={!host}
            title="Open this course's class list in Brightspace"
          >
            <span className="icon"><i className="fas fa-address-book"></i></span>
            <span>Open in Brightspace</span>
          </button>
        </div>
      </div>

      {error && (
        <div className={`notification is-warning is-light py-3 mt-3 ${people.length > 0 ? "mb-3" : "mb-0"}`}>
          <p className="is-size-7">
            Brilliant could not refresh this list. {people.length > 0 ? "Your last saved copy is still here." : error}
          </p>
        </div>
      )}

      {!loading && people.length > 0 && (
        <div className="field mt-4 mb-3">
          <div className="control has-icons-left">
            <input
              className="input is-small"
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Find a classmate by name, email, or role"
              aria-label="Search the class list"
            />
            <span className="icon is-small is-left"><i className="fas fa-search"></i></span>
          </div>
        </div>
      )}

      {loading ? (
        <div className="has-text-centered has-text-grey-light py-5">
          <i className="fas fa-circle-notch fa-spin mr-2"></i>Checking Brightspace…
        </div>
      ) : people.length === 0 ? (
        <p className="has-text-grey is-size-7 mt-3">
          No class-list entries are saved yet. Try Refresh, or open the Brightspace class list if your school limits access.
        </p>
      ) : visiblePeople.length === 0 ? (
        <p className="has-text-grey is-size-7 py-4">No one matches “{query}”.</p>
      ) : (
        <div className="class-list-grid">
          {visiblePeople.map((person) => (
            <article className="class-list-person" key={person.brightspace_user_id}>
              <div className="class-list-avatar" aria-hidden="true">{initials(person)}</div>
              <div className="class-list-person-details">
                <div className="is-flex is-align-items-center is-flex-wrap-wrap" style={{ gap: 5 }}>
                  <strong className="is-size-7">{person.display_name}</strong>
                  {person.is_current_user && <span className="tag is-primary is-light is-small">You</span>}
                  {person.role_name && (
                    <span className={`tag is-small ${isLearner(person) ? "is-light" : "is-info is-light"}`}>
                      {person.role_name}
                    </span>
                  )}
                </div>
                {person.pronouns && <p className="is-size-7 has-text-grey">{person.pronouns}</p>}
                {person.email && (
                  <div className="class-list-email-row">
                    <span className="is-size-7 has-text-grey class-list-email">{person.email}</span>
                    <button
                      type="button"
                      className="button is-white is-small class-list-copy"
                      onClick={() => copyEmail(person.email!)}
                      title={`Copy ${person.display_name}'s email address`}
                      aria-label={`Copy ${person.display_name}'s email address`}
                    >
                      <span className="icon is-small"><i className="far fa-copy"></i></span>
                    </button>
                  </div>
                )}
              </div>
            </article>
          ))}
        </div>
      )}

      {people.length > 0 && (
        <p className="help mt-3">
          Saved on this device from Brightspace. Refresh updates names, roles, and visible email addresses.
        </p>
      )}
    </section>
  );
}
