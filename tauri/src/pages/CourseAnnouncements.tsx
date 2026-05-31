import { useCallback, useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { Notification } from "../types";
import HeaderBand from "../components/HeaderBand";
import RichText from "../components/RichText";
import SyntheticTaskModal from "../components/SyntheticTaskModal";

// Per-course announcements view, ported from the old Sinatra app's
// /course/:id/announcements route. Notifications are already course-scoped
// in the schema; this just filters and lets the user convert any one into
// a synthetic task in a click (the headline "Reading 12 due Friday"
// announcement becomes a to-do without re-typing).

export default function CourseAnnouncements() {
  const { id } = useParams<{ id: string }>();
  const [items, setItems] = useState<Notification[] | null>(null);
  const [showRead, setShowRead] = useState(true);
  const [creatingFrom, setCreatingFrom] = useState<Notification | null>(null);

  const load = useCallback(() => {
    if (!id) return;
    api.listNotifications({ courseId: id, unreadOnly: !showRead })
      .then(setItems)
      .catch(() => setItems([]));
  }, [id, showRead]);

  useEffect(() => { load(); }, [load]);

  // Re-fetch whenever the backend emits a notifications update: mark-read
  // from the Dashboard Notifications view, mark-all-read, and the sync
  // engine all fire this event. Without it, resolving the same alert from
  // the Notifications page leaves it visibly unresolved here.
  useEffect(() => {
    const unlistenP = listen("notifications:updated", () => { load(); });
    return () => { unlistenP.then((fn) => fn()).catch(() => {}); };
  }, [load]);

  async function markRead(n: Notification) {
    await api.markNotificationRead(n.id);
    load();
  }

  function announcementToTaskName(n: Notification): string {
    return n.title.replace(/^announcement[: ]\s*/i, "").trim();
  }
  function announcementToTaskDescription(n: Notification): string {
    const body = (n.body ?? "").trim();
    const dateLine = n.date ? `Posted ${new Date(n.date).toLocaleDateString()}\n\n` : "";
    return `From announcement: **${n.title}**\n\n${dateLine}${body}`;
  }

  return (
    <div>
      {id && <HeaderBand courseId={id} />}

      <div className="box">
        <div className="is-flex is-justify-content-space-between is-align-items-center mb-3">
          <h2 className="title is-5 mb-0">
            <i className="fas fa-bullhorn mr-2 has-text-grey"></i>Announcements
          </h2>
          <label className="checkbox is-size-7">
            <input
              type="checkbox"
              checked={showRead}
              onChange={(e) => setShowRead(e.target.checked)}
              className="mr-1"
            />
            Show read
          </label>
        </div>

        {items === null ? (
          <p className="has-text-grey py-3 is-size-7">Loading…</p>
        ) : items.length === 0 ? (
          <p className="has-text-grey py-3 is-size-7">
            {showRead ? "No announcements for this course yet." : "No unread announcements."}
          </p>
        ) : (
          <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
            {items.map((n) => (
              <li
                key={n.id}
                style={{
                  borderTop: "1px solid #eee",
                  padding: "12px 0",
                  opacity: n.is_read ? 0.7 : 1,
                }}
              >
                <div className="is-flex is-align-items-baseline" style={{ gap: 8 }}>
                  <strong style={{ flex: "1 1 auto" }}>{n.title}</strong>
                  {n.date && (
                    <span className="has-text-grey is-size-7" style={{ flex: "0 0 auto" }}>
                      {new Date(n.date).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" })}
                    </span>
                  )}
                </div>
                {n.body && <RichText content={n.body} className="is-size-7" />}
                <div className="is-flex mt-2" style={{ gap: 6 }}>
                  <button
                    className="button is-small is-primary is-light"
                    onClick={() => setCreatingFrom(n)}
                    title="Turn this announcement into a synthetic task"
                  >
                    <span className="icon is-small"><i className="fas fa-plus"></i></span>
                    <span>Create task</span>
                  </button>
                  {!n.is_read && (
                    <button className="button is-small is-light" onClick={() => markRead(n)}>
                      <span className="icon is-small"><i className="fas fa-check"></i></span>
                      <span>Mark read</span>
                    </button>
                  )}
                  {n.url && (
                    <button
                      className="button is-small is-light"
                      onClick={() => api.openUrl(n.url!).catch(() => {})}
                      title="Open the source in Brightspace"
                    >
                      <span className="icon is-small"><i className="fas fa-up-right-from-square"></i></span>
                      <span>Open</span>
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>

      {id && (
        <SyntheticTaskModal
          courseId={id}
          open={creatingFrom !== null}
          onClose={() => setCreatingFrom(null)}
          onCreated={async () => {
            if (creatingFrom) await markRead(creatingFrom);
            setCreatingFrom(null);
          }}
          initialName={creatingFrom ? announcementToTaskName(creatingFrom) : undefined}
          initialDescription={creatingFrom ? announcementToTaskDescription(creatingFrom) : undefined}
        />
      )}
    </div>
  );
}
