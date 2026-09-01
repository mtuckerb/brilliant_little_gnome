// Helpers for building markdown out of strings we did not author — module
// titles, file names, and Brightspace URLs all arrive verbatim and can carry
// characters that mean something to the markdown parser. Anything that ends
// up inside a task note (RichText renders those as markdown) should go
// through these rather than straight into a template literal.

// Only the characters that change meaning *inline*: emphasis, code, links,
// and raw HTML. Line-start syntax (`#`, `-`, `+`) and table pipes are left
// alone deliberately — this escaped text is also what the user reads in the
// task modal's textarea, and `Levenson\-2017` there is worse than the
// rendering it would protect. Backslash comes first in the class so it does
// not double-escape what follows. `!` needs no entry: it only makes an image
// when followed by `[`, which is escaped.
const MD_SPECIAL_RE = /[\\`*_[\]<>]/g;

/**
 * Escape markdown metacharacters in text that should render literally. A
 * title like `Ch. 3 [cont.` would otherwise swallow the link it labels, and
 * `week_1_notes` would come out half-italic.
 */
export function mdEscape(text: string): string {
  return text.replace(MD_SPECIAL_RE, (c) => `\\${c}`);
}

/**
 * Build a markdown link with both halves made safe. The destination uses the
 * angle-bracket form, which tolerates spaces and unbalanced parens; the only
 * characters that can still end it early are `<` and `>`, so those are
 * percent-encoded.
 */
export function mdLink(text: string, url: string): string {
  const dest = url.replace(/</g, "%3C").replace(/>/g, "%3E");
  return `[${mdEscape(text)}](<${dest}>)`;
}
