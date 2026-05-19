import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { DiscussionForum, DiscussionTopic } from "../types";
import CourseHeader from "../components/CourseHeader";
import BrightspaceLink, { useBrightspaceHost } from "../components/BrightspaceLink";
import { discussionForumUrl, discussionTopicUrl } from "../lib/brightspace";
import RichText from "../components/RichText";
import { useToast } from "../components/ToastProvider";

export default function Discussions() {
  const { id: courseId } = useParams<{ id: string }>();
  const [forums, setForums] = useState<DiscussionForum[] | null>(null);
  const [topics, setTopics] = useState<DiscussionTopic[]>([]);
  const [markingAll, setMarkingAll] = useState(false);
  const bsHost = useBrightspaceHost();
  const toast = useToast();

  async function onMarkAllCourseRead() {
    if (!courseId || markingAll) return;
    setMarkingAll(true);
    toast.show("Marking every discussion in this course read on Brightspace…", "is-info", 4000);
    try {
      const r = await api.markCourseDiscussionsRead(courseId);
      if (r.failed === 0) {
        toast.show(`Marked ${r.marked} post${r.marked === 1 ? "" : "s"} read across the course.`, "is-success");
      } else {
        toast.show(`Marked ${r.marked} read; ${r.failed} failed.`, "is-warning", 6000);
      }
    } catch (e) {
      toast.show(`Mark-read failed: ${String((e as { message?: string })?.message ?? e)}`, "is-danger", 6000);
    } finally {
      setMarkingAll(false);
    }
  }

  useEffect(() => {
    if (!courseId) return;
    Promise.all([api.listForums(courseId), api.listTopics(courseId)]).then(([f, t]) => {
      setForums(f);
      setTopics(t);
    });
  }, [courseId]);

  if (!forums) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  return (
    <div>
      {courseId && <CourseHeader courseId={courseId} />}
      <div className="is-flex is-align-items-center is-flex-wrap-wrap mb-3" style={{ gap: 8 }}>
        <h1 className="title mb-0"><i className="fas fa-comments mr-2"></i>Discussions</h1>
        <span style={{ flex: "1 1 auto" }} />
        <button
          className="button is-small is-primary is-light"
          onClick={onMarkAllCourseRead}
          disabled={markingAll}
          title="Mark every post in every topic of this course as read on Brightspace"
        >
          <span className="icon is-small"><i className={`fas ${markingAll ? "fa-circle-notch fa-spin" : "fa-check-double"}`}></i></span>
          <span>{markingAll ? "Marking…" : "Mark course read"}</span>
        </button>
      </div>
      {forums.length === 0 && <p className="has-text-grey">No discussion forums.</p>}
      {forums.map((f) => {
        const ft = topics.filter((t) => t.forum_id === f.brightspace_id);
        return (
          <div className="box mb-3" key={f.id}>
            <div className="is-flex is-align-items-center" style={{ gap: 6 }}>
              <h2 className="subtitle mb-0">{f.name}</h2>
              {bsHost && courseId && (
                <BrightspaceLink url={discussionForumUrl(bsHost, courseId, f.brightspace_id)} label="Open this forum in Brightspace" />
              )}
            </div>
            {f.description && <RichText content={f.description} bulmaContent={false} className="is-size-7 has-text-grey-dark mb-2" />}
            {ft.length === 0 ? (
              <p className="has-text-grey-light is-size-7">No topics.</p>
            ) : (
              <ul>
                {ft.map((t) => (
                  <li key={t.id} className="py-1" style={{ borderTop: "1px solid #eee" }}>
                    <div className="is-flex is-justify-content-space-between is-align-items-center">
                      <div className="is-flex is-align-items-center" style={{ gap: 4 }}>
                        <Link to={`/course/${courseId}/discussions/${t.brightspace_id}`}>
                          <strong>{t.name}</strong>
                        </Link>
                        {bsHost && courseId && (
                          <BrightspaceLink url={discussionTopicUrl(bsHost, courseId, t.brightspace_id)} label="Open this discussion in Brightspace" />
                        )}
                      </div>
                      <span className="has-text-grey is-size-7">
                        {t.thread_count != null && <>{t.thread_count} threads · </>}
                        {t.post_count != null && <>{t.post_count} posts</>}
                      </span>
                    </div>
                    {t.last_post_date && <div className="is-size-7 has-text-grey">last post {new Date(t.last_post_date).toLocaleString()}</div>}
                  </li>
                ))}
              </ul>
            )}
          </div>
        );
      })}
    </div>
  );
}
