import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { DiscussionForum, DiscussionTopic } from "../types";

export default function Discussions() {
  const { id: courseId } = useParams<{ id: string }>();
  const [forums, setForums] = useState<DiscussionForum[] | null>(null);
  const [topics, setTopics] = useState<DiscussionTopic[]>([]);

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
      <h1 className="title"><i className="fas fa-comments mr-2"></i>Discussions</h1>
      {forums.length === 0 && <p className="has-text-grey">No discussion forums.</p>}
      {forums.map((f) => {
        const ft = topics.filter((t) => t.forum_id === f.brightspace_id);
        return (
          <div className="box mb-3" key={f.id}>
            <h2 className="subtitle mb-2">{f.name}</h2>
            {f.description && <p className="is-size-7 has-text-grey-dark mb-2" dangerouslySetInnerHTML={{ __html: f.description }} />}
            {ft.length === 0 ? (
              <p className="has-text-grey-light is-size-7">No topics.</p>
            ) : (
              <ul>
                {ft.map((t) => (
                  <li key={t.id} className="py-1" style={{ borderTop: "1px solid #eee" }}>
                    <div className="is-flex is-justify-content-space-between">
                      <Link to={`/course/${courseId}/discussions/${t.brightspace_id}`}>
                        <strong>{t.name}</strong>
                      </Link>
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
