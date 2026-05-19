import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

// Renders any content body — Brightspace's HTML (`<p>…</p>`), a user's
// hand-typed markdown, or plain text — with one component. Detection is
// dumb-but-correct: if the input contains an opening tag, treat it as HTML;
// otherwise treat it as markdown. Brightspace always ships HTML, so its
// surfaces fall through to the HTML branch with no behavior change; the
// markdown branch is what lets user-authored content (synthetic task
// descriptions, notes) render nicely without anyone having to remember
// which renderer applies where.
//
// Security note: existing call sites already trusted Brightspace HTML via
// dangerouslySetInnerHTML, so the HTML branch is no worse than before.
// react-markdown's default config is HTML-safe (no raw HTML embedded in
// markdown is rendered) — sufficient for first-party user input.

interface Props {
  content: string | null | undefined;
  /// Bulma's `content` class adds list / heading styles. Most callers want
  /// it; pass `false` for inline-style surfaces (small descriptions, etc).
  bulmaContent?: boolean;
  className?: string;
  /// Override the auto-detection — useful if you know what you have.
  format?: "auto" | "html" | "markdown";
}

// Anything that looks like an opening tag (letters after `<`, with optional
// attributes) counts as HTML. Plain "<3" / "<- arrow" don't match.
const HTML_TAG_RE = /<\s*[a-zA-Z][^>]*>/;

export default function RichText({
  content,
  bulmaContent = true,
  className,
  format = "auto",
}: Props) {
  if (!content) return null;

  const isHtml = format === "html" || (format === "auto" && HTML_TAG_RE.test(content));
  const cls = `${bulmaContent ? "content " : ""}${className ?? ""}`.trim();

  if (isHtml) {
    return <div className={cls || undefined} dangerouslySetInnerHTML={{ __html: content }} />;
  }
  return (
    <div className={cls || undefined}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
    </div>
  );
}
