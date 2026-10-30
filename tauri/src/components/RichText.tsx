import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "../api";
import { isZoteroDocumentUrl } from "../lib/zotero";

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
  /// Course-scoped destinations for sources synced after these notes were written.
  resolveLink?: (url: string) => string;
}

// Require the tag name to end where HTML permits it to end. In particular,
// Markdown's angle-bracket link destination (`[label](<https://…>)`) is not
// an HTML tag: the colon immediately after `https` rules it out.
const HTML_TAG_RE = /<\s*[a-zA-Z][a-zA-Z0-9-]*(?:\s[^>]*)?\/?\s*>/;

export function looksLikeHtml(content: string): boolean {
  return HTML_TAG_RE.test(content);
}

export default function RichText({
  content,
  bulmaContent = true,
  className,
  format = "auto",
  resolveLink,
}: Props) {
  if (!content) return null;

  const isHtml = format === "html" || (format === "auto" && looksLikeHtml(content));
  const cls = `${bulmaContent ? "content " : ""}${className ?? ""}`.trim();

  function openZotero(event: React.MouseEvent<HTMLDivElement>) {
    const link = (event.target as Element).closest("a[href]");
    const url = link?.getAttribute("href");
    if (!url || !isZoteroDocumentUrl(url)) return;
    event.preventDefault();
    void api.openUrl(url).catch((error) => alert(`Could not open Zotero: ${String(error)}`));
  }

  if (isHtml) {
    let html = content;
    if (resolveLink) {
      const fragment = document.createElement("template");
      fragment.innerHTML = content;
      for (const link of fragment.content.querySelectorAll("a[href]")) {
        const href = link.getAttribute("href")!;
        const destination = resolveLink(href);
        if (destination !== href) link.setAttribute("href", destination);
      }
      html = fragment.innerHTML;
    }
    return <div className={cls || undefined} onClick={openZotero} dangerouslySetInnerHTML={{ __html: html }} />;
  }
  return (
    <div className={cls || undefined} onClick={openZotero}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} urlTransform={(url, key, node) => {
        const destination = node.tagName === "a" && resolveLink ? resolveLink(url) : url;
        return isZoteroDocumentUrl(destination) ? destination : defaultUrlTransform(destination);
      }}>{content}</ReactMarkdown>
    </div>
  );
}
