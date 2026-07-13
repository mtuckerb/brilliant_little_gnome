//! Minimal, dependency-free HTML → Markdown converter (built on `scraper`,
//! already a dependency). Not a general-purpose converter — it targets the
//! subset Brightspace content pages, overviews, and announcements actually use:
//! headings, paragraphs, line breaks, bold/italic, links, images, lists,
//! blockquotes, code, and horizontal rules. Anything else falls through to its
//! text content so nothing is lost. The output is meant to be readable and
//! searchable in Obsidian, not a byte-perfect round-trip.

use ego_tree::NodeRef;
use scraper::{Html, Node};

pub fn html_to_markdown(html: &str) -> String {
    let doc = Html::parse_fragment(html);
    let mut out = String::new();
    for child in doc.tree.root().children() {
        walk(child, &mut out, 0);
    }
    normalize(&out)
}

fn walk(node: NodeRef<Node>, out: &mut String, list_depth: usize) {
    match node.value() {
        Node::Text(t) => out.push_str(&collapse_ws(&t.text)),
        Node::Element(el) => {
            let tag = el.name().to_ascii_lowercase();
            match tag.as_str() {
                "script" | "style" | "head" | "noscript" => {}
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let level: usize = tag[1..].parse().unwrap_or(1);
                    out.push_str("\n\n");
                    out.push_str(&"#".repeat(level));
                    out.push(' ');
                    walk_children(node, out, list_depth);
                    out.push_str("\n\n");
                }
                "p" | "div" | "section" | "article" => {
                    out.push_str("\n\n");
                    walk_children(node, out, list_depth);
                    out.push_str("\n\n");
                }
                "br" => out.push_str("  \n"),
                "hr" => out.push_str("\n\n---\n\n"),
                "strong" | "b" => {
                    out.push_str("**");
                    walk_children(node, out, list_depth);
                    out.push_str("**");
                }
                "em" | "i" => {
                    out.push('*');
                    walk_children(node, out, list_depth);
                    out.push('*');
                }
                "code" => {
                    out.push('`');
                    walk_children(node, out, list_depth);
                    out.push('`');
                }
                "pre" => {
                    out.push_str("\n\n```\n");
                    walk_children(node, out, list_depth);
                    out.push_str("\n```\n\n");
                }
                "blockquote" => {
                    out.push_str("\n\n> ");
                    walk_children(node, out, list_depth);
                    out.push_str("\n\n");
                }
                "a" => {
                    let href = el.attr("href").unwrap_or("").trim();
                    out.push('[');
                    walk_children(node, out, list_depth);
                    out.push_str("](");
                    out.push_str(href);
                    out.push(')');
                }
                "img" => {
                    let src = el.attr("src").unwrap_or("").trim();
                    let alt = el.attr("alt").unwrap_or("").trim();
                    out.push_str(&format!("![{}]({})", alt, src));
                }
                "ul" | "ol" => {
                    out.push('\n');
                    let ordered = tag == "ol";
                    let mut i = 1;
                    for child in node.children() {
                        if let Node::Element(ce) = child.value() {
                            if ce.name().eq_ignore_ascii_case("li") {
                                out.push('\n');
                                out.push_str(&"  ".repeat(list_depth));
                                if ordered {
                                    out.push_str(&format!("{}. ", i));
                                    i += 1;
                                } else {
                                    out.push_str("- ");
                                }
                                walk_children(child, out, list_depth + 1);
                            }
                        }
                    }
                    out.push('\n');
                }
                // Tables and everything else: keep the text content.
                _ => walk_children(node, out, list_depth),
            }
        }
        _ => {}
    }
}

fn walk_children(node: NodeRef<Node>, out: &mut String, list_depth: usize) {
    for child in node.children() {
        walk(child, out, list_depth);
    }
}

fn collapse_ws(s: &str) -> String {
    // Collapse runs of whitespace (incl. newlines) to single spaces so source
    // HTML indentation doesn't leak into the markdown.
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out
}

fn normalize(s: &str) -> String {
    // Collapse 3+ newlines to 2, trim trailing spaces per line, trim ends.
    let mut lines: Vec<String> = s.lines().map(|l| l.trim_end().to_string()).collect();
    // Drop leading/trailing blank lines.
    while lines.first().map_or(false, |l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().map_or(false, |l| l.is_empty()) {
        lines.pop();
    }
    let mut out = String::new();
    let mut blanks = 0;
    for l in lines {
        if l.is_empty() {
            blanks += 1;
            if blanks <= 1 {
                out.push('\n');
            }
        } else {
            blanks = 0;
            out.push_str(&l);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::html_to_markdown;

    #[test]
    fn headings_paragraphs_and_inline() {
        let md = html_to_markdown("<h2>Week 1</h2><p>Read the <strong>syllabus</strong> and <a href=\"http://x.com\">this link</a>.</p>");
        assert!(md.contains("## Week 1"), "{md}");
        assert!(md.contains("**syllabus**"), "{md}");
        assert!(md.contains("[this link](http://x.com)"), "{md}");
    }

    #[test]
    fn lists_render_as_bullets() {
        let md = html_to_markdown("<ul><li>First</li><li>Second</li></ul>");
        assert!(md.contains("- First"), "{md}");
        assert!(md.contains("- Second"), "{md}");
    }

    #[test]
    fn ordered_list_numbers() {
        let md = html_to_markdown("<ol><li>One</li><li>Two</li></ol>");
        assert!(md.contains("1. One"), "{md}");
        assert!(md.contains("2. Two"), "{md}");
    }

    #[test]
    fn scripts_and_styles_are_dropped() {
        let md = html_to_markdown("<p>Hi</p><script>alert(1)</script><style>.x{}</style>");
        assert!(md.contains("Hi"));
        assert!(!md.contains("alert"), "{md}");
        assert!(!md.contains(".x{"), "{md}");
    }

    #[test]
    fn image_becomes_markdown_image() {
        let md = html_to_markdown("<img src=\"/img/a.png\" alt=\"Diagram\">");
        assert_eq!(md.trim(), "![Diagram](/img/a.png)");
    }
}
