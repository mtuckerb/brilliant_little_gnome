import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { DiscussionPost, DiscussionTopic as DTopic } from "../types";

interface PostNode extends DiscussionPost {
  children: PostNode[];
}

function buildTree(posts: DiscussionPost[]): PostNode[] {
  const byId = new Map<string, PostNode>();
  posts.forEach((p) => byId.set(p.post_id, { ...p, children: [] }));
  const roots: PostNode[] = [];
  byId.forEach((node) => {
    const parent = node.parent_post_id ? byId.get(node.parent_post_id) : null;
    if (parent) parent.children.push(node);
    else roots.push(node);
  });
  const sortRec = (nodes: PostNode[]) => {
    nodes.sort((a, b) => {
      if (a.is_pinned !== b.is_pinned) return a.is_pinned ? -1 : 1;
      const da = a.posted_at ? Date.parse(a.posted_at) : 0;
      const db = b.posted_at ? Date.parse(b.posted_at) : 0;
      return da - db;
    });
    nodes.forEach((n) => sortRec(n.children));
  };
  sortRec(roots);
  return roots;
}

function PostView({ node, depth }: { node: PostNode; depth: number }) {
  const [collapsed, setCollapsed] = useState(false);
  return (
    <div
      className="box mb-2"
      style={{ marginLeft: depth * 20, padding: "0.6rem 0.8rem" }}
    >
      <div className="is-flex is-justify-content-space-between is-align-items-center mb-1">
        <div>
          {node.is_pinned && (
            <span className="tag is-warning is-small mr-2">
              <i className="fas fa-thumbtack mr-1"></i>pinned
            </span>
          )}
          {node.subject && <strong>{node.subject}</strong>}
          <span className="is-size-7 has-text-grey ml-2">
            {node.author_name ?? "unknown"}
            {node.posted_at && <> · {new Date(node.posted_at).toLocaleString()}</>}
          </span>
        </div>
        {node.children.length > 0 && (
          <button
            className="button is-small is-white"
            onClick={() => setCollapsed((c) => !c)}
            title={collapsed ? "expand replies" : "collapse replies"}
          >
            <span className="icon is-small">
              <i className={`fas fa-chevron-${collapsed ? "right" : "down"}`}></i>
            </span>
            <span className="ml-1 is-size-7">{node.children.length}</span>
          </button>
        )}
      </div>
      {node.body_html && (
        <div
          className="content is-small"
          dangerouslySetInnerHTML={{ __html: node.body_html }}
        />
      )}
      {!collapsed &&
        node.children.map((c) => (
          <PostView key={c.post_id} node={c} depth={depth + 1} />
        ))}
    </div>
  );
}

export default function DiscussionTopicPage() {
  const { id: courseId, topicId } = useParams<{ id: string; topicId: string }>();
  const [topic, setTopic] = useState<DTopic | null>(null);
  const [posts, setPosts] = useState<DiscussionPost[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!courseId || !topicId) return;
    api
      .listTopics(courseId)
      .then((all) => setTopic(all.find((t) => t.brightspace_id === topicId) ?? null));
    api
      .listTopicPosts(courseId, topicId)
      .then(setPosts)
      .catch((e) => setError(String(e)));
  }, [courseId, topicId]);

  const tree = useMemo(() => (posts ? buildTree(posts) : []), [posts]);

  return (
    <div>
      <div className="mb-3">
        <Link to={`/course/${courseId}/discussions`} className="is-size-7">
          <i className="fas fa-arrow-left mr-1"></i>back to discussions
        </Link>
      </div>
      <h1 className="title is-4">
        <i className="fas fa-comments mr-2"></i>
        {topic?.name ?? "Topic"}
      </h1>
      {topic?.description && (
        <div
          className="content is-small has-text-grey-dark mb-3"
          dangerouslySetInnerHTML={{ __html: topic.description }}
        />
      )}
      {error && <div className="notification is-danger is-light">{error}</div>}
      {posts === null && !error && (
        <div className="has-text-centered py-6">
          <span className="icon is-large has-text-primary">
            <i className="fas fa-circle-notch fa-spin fa-3x"></i>
          </span>
        </div>
      )}
      {posts && posts.length === 0 && (
        <p className="has-text-grey">No posts in this topic yet.</p>
      )}
      {tree.map((n) => (
        <PostView key={n.post_id} node={n} depth={0} />
      ))}
    </div>
  );
}
