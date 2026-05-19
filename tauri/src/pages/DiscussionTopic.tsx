import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import type { DiscussionPost, DiscussionTopic as DTopic } from "../types";
import CourseHeader from "../components/CourseHeader";
import BrightspaceLink, { useBrightspaceHost } from "../components/BrightspaceLink";
import { discussionTopicUrl } from "../lib/brightspace";
import RichText from "../components/RichText";
import { useToast } from "../components/ToastProvider";

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

function PostView({ node, depth, collapseSignal }: { node: PostNode; depth: number; collapseSignal: number }) {
  // Every post starts collapsed — page opens as a stack of one-liners
  // (author / title / date / reply count). Click the chevron (or the
  // header itself) to expand the body + replies.
  const [collapsed, setCollapsed] = useState(true);
  // External collapse/expand-all is driven by a signed counter the parent
  // bumps. Positive = collapse everything, negative = expand everything.
  useEffect(() => {
    if (collapseSignal === 0) return;
    setCollapsed(collapseSignal > 0);
  }, [collapseSignal]);

  const author = node.author_name && node.author_name.trim() ? node.author_name : "(no author)";

  return (
    <div
      className="box mb-2"
      style={{ marginLeft: depth * 20, padding: "0.6rem 0.8rem" }}
    >
      <div
        className="is-flex is-justify-content-space-between is-align-items-center mb-1"
        onClick={() => setCollapsed((c) => !c)}
        style={{ cursor: "pointer", userSelect: "none" }}
        title={collapsed ? "Click to expand" : "Click to collapse"}
      >
        <div>
          {node.is_pinned && (
            <span className="tag is-warning is-small mr-2">
              <i className="fas fa-thumbtack mr-1"></i>pinned
            </span>
          )}
          {node.subject && <strong>{node.subject}</strong>}
          <span className="is-size-7 has-text-grey ml-2">
            {author}
            {node.posted_at && <> · {new Date(node.posted_at).toLocaleString()}</>}
            {node.children.length > 0 && (
              <> · {node.children.length} repl{node.children.length === 1 ? "y" : "ies"}</>
            )}
          </span>
        </div>
        <span className="icon is-small has-text-grey">
          <i className={`fas fa-chevron-${collapsed ? "right" : "down"}`}></i>
        </span>
      </div>
      {!collapsed && node.body_html && <RichText content={node.body_html} className="is-small" />}
      {!collapsed &&
        node.children.map((c) => (
          <PostView key={c.post_id} node={c} depth={depth + 1} collapseSignal={collapseSignal} />
        ))}
    </div>
  );
}

export default function DiscussionTopicPage() {
  const { id: courseId, topicId } = useParams<{ id: string; topicId: string }>();
  const [topic, setTopic] = useState<DTopic | null>(null);
  const [posts, setPosts] = useState<DiscussionPost[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Positive bumps collapse all; negative bumps expand all. Posts watch
  // the change in this counter, not its sign-on-mount, so re-renders
  // don't re-collapse arbitrarily.
  const [collapseSignal, setCollapseSignal] = useState(0);
  const [markingRead, setMarkingRead] = useState(false);
  const toast = useToast();

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
  const bsHost = useBrightspaceHost();

  async function onMarkAllRead() {
    if (!courseId || !topicId || markingRead) return;
    setMarkingRead(true);
    toast.show("Marking topic read in Brightspace…", "is-info", 3000);
    try {
      const r = await api.markTopicRead(courseId, topicId);
      if (r.failed === 0) {
        toast.show(`Marked ${r.marked} post${r.marked === 1 ? "" : "s"} read.`, "is-success");
      } else {
        toast.show(`Marked ${r.marked} read; ${r.failed} failed.`, "is-warning", 6000);
      }
    } catch (e) {
      toast.show(`Mark-read failed: ${String((e as { message?: string })?.message ?? e)}`, "is-danger", 6000);
    } finally {
      setMarkingRead(false);
    }
  }

  return (
    <div>
      {courseId && <CourseHeader courseId={courseId} />}
      <div className="mb-3">
        <Link to={`/course/${courseId}/discussions`} className="is-size-7">
          <i className="fas fa-arrow-left mr-1"></i>back to discussions
        </Link>
      </div>
      <div className="is-flex is-align-items-center is-flex-wrap-wrap" style={{ gap: 8 }}>
        <h1 className="title is-4 mb-0">
          <i className="fas fa-comments mr-2"></i>
          {topic?.name ?? "Topic"}
        </h1>
        {bsHost && courseId && topicId && (
          <BrightspaceLink url={discussionTopicUrl(bsHost, courseId, topicId)} label="Open this discussion in Brightspace (reply there)" />
        )}
        <span style={{ flex: "1 1 auto" }} />
        <button
          className="button is-small is-light"
          onClick={() => setCollapseSignal((c) => c > 0 ? c + 1 : 1)}
          title="Collapse every thread"
        >
          <span className="icon is-small"><i className="fas fa-chevron-up"></i></span>
          <span>Collapse all</span>
        </button>
        <button
          className="button is-small is-light"
          onClick={() => setCollapseSignal((c) => c < 0 ? c - 1 : -1)}
          title="Expand every thread"
        >
          <span className="icon is-small"><i className="fas fa-chevron-down"></i></span>
          <span>Expand all</span>
        </button>
        <button
          className="button is-small is-primary is-light"
          onClick={onMarkAllRead}
          disabled={markingRead}
          title="Mark every post in this topic as read on Brightspace"
        >
          <span className="icon is-small"><i className={`fas ${markingRead ? "fa-circle-notch fa-spin" : "fa-check-double"}`}></i></span>
          <span>{markingRead ? "Marking…" : "Mark all read"}</span>
        </button>
      </div>
      {topic?.description && (
        <RichText content={topic.description} className="is-small has-text-grey-dark mb-3" />
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
        <PostView key={n.post_id} node={n} depth={0} collapseSignal={collapseSignal} />
      ))}
    </div>
  );
}
