import { useEffect, useState, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { Notification } from "../types";

export default function Notifications() {
  const [items, setItems] = useState<Notification[] | null>(null);
  const [unreadOnly, setUnreadOnly] = useState(false);

  const load = useCallback(() => {
    api.listNotifications({ unreadOnly, limit: 100 }).then(setItems);
  }, [unreadOnly]);

  useEffect(() => { load(); }, [load]);

  // Re-fetch whenever the backend emits a notifications update: mark-read
  // from CourseAnnouncements, mark-all-read, and the sync engine all fire
  // this event. Without it, resolving an alert from the course view leaves
  // the same alert visibly unresolved on the Dashboard's Notifications
  // page until the user navigates away and back.
  useEffect(() => {
    const unlistenP = listen("notifications:updated", () => { load(); });
    return () => { unlistenP.then((fn) => fn()).catch(() => {}); };
  }, [load]);

  if (!items) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  return (
    <div>
      <div className="level">
        <div className="level-left"><h1 className="title"><i className="fas fa-bell mr-2"></i>Notifications</h1></div>
        <div className="level-right">
          <button className="button is-small mr-2" onClick={() => setUnreadOnly((s) => !s)}>
            {unreadOnly ? "Show all" : "Unread only"}
          </button>
          <button className="button is-small" onClick={() => api.markAllNotificationsRead().then(load)}>
            Mark all read
          </button>
        </div>
      </div>
      <div className="box">
        {items.length === 0 && <p className="has-text-grey py-4 has-text-centered">No notifications.</p>}
        {items.map((n) => (
          <div key={n.id} className={`mb-3 p-3 ${n.is_read ? "" : "has-background-info-light"}`}
               style={{ borderLeft: n.is_read ? "3px solid #eee" : "3px solid #739AC3" }}>
            <div className="is-flex is-justify-content-space-between is-align-items-start">
              <div style={{ flex: 1 }}>
                <p className="has-text-weight-bold">{n.title}</p>
                {n.body && <p className="is-size-7 has-text-grey-dark mt-1">{n.body}</p>}
                <p className="is-size-7 has-text-grey mt-1">
                  {n.course_name && <span>{n.course_name} · </span>}
                  {n.date && new Date(n.date).toLocaleString()}
                </p>
              </div>
              {!n.is_read && (
                <button className="button is-ghost is-small" onClick={() => api.markNotificationRead(n.id).then(load)} title="Mark read">
                  <i className="fas fa-check"></i>
                </button>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
