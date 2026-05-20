//! Token-tree pre-processing applied before lowering.
//!
//! Today this exists to recognise inline `<a href="…">…</a>` HTML and
//! rewrite it into a real [`Token::Link`], so the renderer's normal
//! link path (clickable annotation + the [`super::postprocess`]
//! tooltip injector) carries it the rest of the way. Block-form `<a>`
//! (the tag alone on a line, wrapping content via blank-line breaks)
//! is still treated as a raw HTML block — the inline form is the
//! common case and the only one covered here.

use crate::markdown::Token;

use super::lower::parse_html_attrs;

/// Walk the token tree and replace every inline `<a href="…">…</a>`
/// pair with a `Token::Link` carrying the parsed `href` (and optional
/// `title`). Unclosed openers and nested `<a>` runs are left as-is so
/// they degrade to the existing pass-through behavior.
pub fn rewrite_html_anchors(tokens: &mut Vec<Token>) {
    for t in tokens.iter_mut() {
        descend(t);
    }
    *tokens = pair_anchors(std::mem::take(tokens));
}

fn descend(tok: &mut Token) {
    match tok {
        Token::Heading(content, _)
        | Token::StrongEmphasis(content)
        | Token::Strikethrough(content)
        | Token::Highlight(content)
        | Token::BlockQuote(content)
        | Token::ListItem { content, .. }
        | Token::Link { content, .. }
        | Token::FootnoteDefinition { content, .. }
        | Token::InlineFootnote { content, .. } => rewrite_html_anchors(content),
        Token::Emphasis { content, .. } => rewrite_html_anchors(content),
        Token::Image { alt, .. } => rewrite_html_anchors(alt),
        Token::Admonition { title, body, .. } => {
            if let Some(t) = title {
                rewrite_html_anchors(t);
            }
            rewrite_html_anchors(body);
        }
        Token::Table { headers, rows, .. } => {
            for cell in headers {
                rewrite_html_anchors(&mut cell.content);
            }
            for row in rows {
                for cell in row {
                    rewrite_html_anchors(&mut cell.content);
                }
            }
        }
        Token::DefinitionList { entries } => {
            for e in entries {
                rewrite_html_anchors(&mut e.term);
                for d in &mut e.definitions {
                    rewrite_html_anchors(d);
                }
            }
        }
        _ => {}
    }
}

fn pair_anchors(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut iter = tokens.into_iter().peekable();
    while let Some(tok) = iter.next() {
        let opener = match &tok {
            Token::HtmlInline(s) => match classify_anchor(s) {
                AnchorTag::OpenWithHref { href, title } => Some((href, title)),
                _ => None,
            },
            _ => None,
        };
        let Some((url, title)) = opener else {
            out.push(tok);
            continue;
        };
        let mut inner: Vec<Token> = Vec::new();
        let mut close_found = false;
        let mut bail = false;
        while let Some(next) = iter.next() {
            // Paragraph break (two or more consecutive Newlines)
            // means the user split the supposed link body across
            // block boundaries. Bail rather than fold the next
            // paragraph into the link body.
            if matches!(next, Token::Newline)
                && matches!(iter.peek(), Some(Token::Newline))
            {
                inner.push(next);
                bail = true;
                break;
            }
            if let Token::HtmlInline(t) = &next {
                match classify_anchor(t) {
                    AnchorTag::CloseA => {
                        close_found = true;
                        break;
                    }
                    AnchorTag::OpenWithHref { .. } | AnchorTag::OpenA => {
                        // Nested `<a>` (or another opener without
                        // href) — HTML doesn't allow it. Restore the
                        // buffered run including the nested opener.
                        inner.push(next);
                        bail = true;
                        break;
                    }
                    AnchorTag::NotAnchor => {}
                }
            }
            inner.push(next);
        }
        if close_found && !bail {
            out.push(Token::Link {
                content: inner,
                url,
                title,
            });
        } else {
            out.push(tok);
            out.extend(inner);
        }
    }
    out
}

enum AnchorTag {
    /// `<a href="…" …>` opener with an `href` attribute. Carries the
    /// raw href value and an optional `title`.
    OpenWithHref { href: String, title: Option<String> },
    /// `<a …>` opener without an `href` attribute (or self-closing
    /// `<a … />`). Treated as an anchor-shaped token for bail
    /// purposes but never starts a link.
    OpenA,
    /// `</a>` close tag.
    CloseA,
    /// Anything else — not an anchor at all.
    NotAnchor,
}

fn classify_anchor(tag: &str) -> AnchorTag {
    let trimmed = tag.trim();
    if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
        return AnchorTag::NotAnchor;
    }
    let inner = trimmed[1..trimmed.len() - 1].trim();
    if let Some(rest) = inner.strip_prefix('/') {
        let name = rest.trim().split_whitespace().next().unwrap_or("");
        return if name.eq_ignore_ascii_case("a") {
            AnchorTag::CloseA
        } else {
            AnchorTag::NotAnchor
        };
    }
    let self_closing = inner.ends_with('/');
    let inner = inner.trim_end_matches('/').trim();
    let name_end = inner
        .find(|c: char| c.is_ascii_whitespace())
        .unwrap_or(inner.len());
    let name = &inner[..name_end];
    if !name.eq_ignore_ascii_case("a") {
        return AnchorTag::NotAnchor;
    }
    if self_closing {
        // `<a … />` has no body, so it cannot wrap content; surface
        // it as an opener for the nested-anchor bail but never start
        // a link with it.
        return AnchorTag::OpenA;
    }
    let attrs = parse_html_attrs(inner[name_end..].trim());
    let href = attrs.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case("href") {
            Some(v.clone())
        } else {
            None
        }
    });
    let title = attrs.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case("title") {
            Some(v.clone())
        } else {
            None
        }
    });
    match href {
        Some(h) => AnchorTag::OpenWithHref { href: h, title },
        None => AnchorTag::OpenA,
    }
}

/// Thin wrappers around `classify_anchor` used by the test helpers.
#[cfg(test)]
fn parse_anchor_open(tag: &str) -> Option<(String, Option<String>)> {
    match classify_anchor(tag) {
        AnchorTag::OpenWithHref { href, title } => Some((href, title)),
        _ => None,
    }
}

#[cfg(test)]
fn is_anchor_close(tag: &str) -> bool {
    matches!(classify_anchor(tag), AnchorTag::CloseA)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::Lexer;

    fn lex(s: &str) -> Vec<Token> {
        Lexer::new(s.to_string()).parse().expect("lex must succeed")
    }

    #[test]
    fn inline_anchor_becomes_link() {
        let mut tokens = lex("Click <a href=\"https://example.com\">here</a>.");
        rewrite_html_anchors(&mut tokens);
        let link = tokens
            .iter()
            .find_map(|t| match t {
                Token::Link {
                    content,
                    url,
                    title,
                } => Some((content, url, title)),
                _ => None,
            })
            .expect("anchor must rewrite to Token::Link");
        assert_eq!(link.1, "https://example.com");
        assert!(link.2.is_none());
        assert!(matches!(link.0.as_slice(), [Token::Text(s)] if s == "here"));
        assert!(
            tokens.iter().all(|t| !matches!(t, Token::HtmlInline(_))),
            "no leftover HtmlInline tokens"
        );
    }

    #[test]
    fn title_attribute_propagates() {
        let mut tokens = lex("See <a href=\"https://example.com\" title=\"Tip\">x</a>!");
        rewrite_html_anchors(&mut tokens);
        let title = tokens.iter().find_map(|t| match t {
            Token::Link { title, .. } => title.clone(),
            _ => None,
        });
        assert_eq!(title.as_deref(), Some("Tip"));
    }

    #[test]
    fn unclosed_anchor_left_as_literal() {
        let mut tokens = lex("hello <a href=\"https://example.com\">world");
        rewrite_html_anchors(&mut tokens);
        assert!(tokens.iter().any(|t| matches!(t, Token::HtmlInline(_))));
        assert!(tokens.iter().all(|t| !matches!(t, Token::Link { .. })));
    }

    #[test]
    fn anchor_without_href_left_alone() {
        let mut tokens = lex("hi <a name=\"x\">y</a>");
        rewrite_html_anchors(&mut tokens);
        assert!(tokens.iter().all(|t| !matches!(t, Token::Link { .. })));
    }

    #[test]
    fn anchor_inside_emphasis() {
        let mut tokens = lex("*click <a href=\"u\">here</a>*");
        rewrite_html_anchors(&mut tokens);
        let em = tokens
            .iter()
            .find_map(|t| match t {
                Token::Emphasis { content, .. } => Some(content),
                _ => None,
            })
            .expect("emphasis present");
        assert!(em.iter().any(|t| matches!(t, Token::Link { .. })));
    }

    fn collect_links(tokens: &[Token]) -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        fn walk(tokens: &[Token], out: &mut Vec<(String, Option<String>)>) {
            for t in tokens {
                match t {
                    Token::Link { content, url, title } => {
                        out.push((url.clone(), title.clone()));
                        walk(content, out);
                    }
                    Token::Heading(c, _)
                    | Token::Emphasis { content: c, .. }
                    | Token::StrongEmphasis(c)
                    | Token::Strikethrough(c)
                    | Token::Highlight(c)
                    | Token::BlockQuote(c)
                    | Token::ListItem { content: c, .. }
                    | Token::FootnoteDefinition { content: c, .. }
                    | Token::InlineFootnote { content: c, .. } => walk(c, out),
                    Token::Image { alt, .. } => walk(alt, out),
                    Token::Table { headers, rows, .. } => {
                        for h in headers {
                            walk(&h.content, out);
                        }
                        for r in rows {
                            for c in r {
                                walk(&c.content, out);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        walk(tokens, &mut out);
        out
    }

    #[test]
    fn single_quoted_href() {
        let mut tokens = lex("see <a href='https://example.com/q?x=1'>x</a>!");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].0, "https://example.com/q?x=1");
    }

    #[test]
    fn uppercase_tag_and_attrs() {
        let mut tokens = lex("see <A HREF=\"https://example.com\" TITLE=\"Tip\">x</A>!");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].0, "https://example.com");
        assert_eq!(links[0].1.as_deref(), Some("Tip"));
    }

    #[test]
    fn self_closing_anchor_is_not_a_link() {
        let mut tokens = lex("see <a href=\"https://example.com\"/> end");
        rewrite_html_anchors(&mut tokens);
        assert!(collect_links(&tokens).is_empty());
    }

    #[test]
    fn multiple_anchors_in_one_paragraph() {
        let mut tokens =
            lex("a <a href=\"u1\">one</a> b <a href=\"u2\" title=\"t\">two</a> c");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].0, "u1");
        assert_eq!(links[1].0, "u2");
        assert_eq!(links[1].1.as_deref(), Some("t"));
    }

    #[test]
    fn nested_anchor_bails_and_leaves_markup() {
        let mut tokens = lex("<a href=\"u\">outer <a href=\"v\">inner</a> tail</a>");
        rewrite_html_anchors(&mut tokens);
        // Pre-existing pass-through behavior wins on invalid nesting:
        // no Link tokens are emitted, every HTML tag survives as
        // HtmlInline so the user sees literal markup.
        assert!(collect_links(&tokens).is_empty());
        let html_count = tokens.iter().filter(|t| matches!(t, Token::HtmlInline(_))).count();
        assert!(html_count >= 2, "expected the original tags to survive: {tokens:?}");
    }

    #[test]
    fn orphan_close_passes_through() {
        let mut tokens = lex("hello </a> world");
        rewrite_html_anchors(&mut tokens);
        assert!(collect_links(&tokens).is_empty());
        assert!(tokens.iter().any(
            |t| matches!(t, Token::HtmlInline(s) if s.eq_ignore_ascii_case("</a>"))
        ));
    }

    #[test]
    fn anchor_with_formatting_body_keeps_children() {
        // The lexer doesn't re-parse content between `<a>` and `</a>`
        // as a self-contained subdocument; emphasis flanking is
        // resolved across the whole paragraph the same way as for
        // markdown without HTML. What we DO require is that whatever
        // tokens the lexer produced between the open/close land inside
        // the synthetic Link's content (no token is dropped or moved
        // out from under the wrapping).
        let pre = lex("see <a href=\"u\">**bold** and *em*</a>!");
        let inline_count = pre
            .iter()
            .filter(|t| !matches!(t, Token::HtmlInline(s) if parse_anchor_open(s).is_some() || is_anchor_close(s)))
            .count();
        let mut tokens = pre.clone();
        rewrite_html_anchors(&mut tokens);
        let link_content = tokens
            .iter()
            .find_map(|t| match t {
                Token::Link { content, .. } => Some(content),
                _ => None,
            })
            .expect("Token::Link expected");
        // The opener and closer disappear; everything else is
        // accounted for between the Link's content and the surrounding
        // siblings.
        let outer_tokens = tokens
            .iter()
            .filter(|t| !matches!(t, Token::Link { .. }))
            .count();
        assert_eq!(outer_tokens + link_content.len(), inline_count);
        // And the visible characters are preserved verbatim.
        let collected = crate::markdown::Token::collect_all_text(&tokens);
        assert!(collected.contains("bold"));
        assert!(collected.contains("em"));
    }

    #[test]
    fn anchor_inside_heading() {
        let mut tokens = lex("# title with <a href=\"u\">link</a>");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].0, "u");
    }

    #[test]
    fn anchor_inside_list_item() {
        let mut tokens = lex("- item with <a href=\"u\">link</a>");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn anchor_inside_table_cell() {
        let mut tokens = lex(
            "| h1 | h2 |\n| --- | --- |\n| <a href=\"u\">link</a> | plain |\n",
        );
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn anchor_inside_blockquote() {
        let mut tokens = lex("> quote with <a href=\"u\">link</a>");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn anchor_with_relative_url_and_fragment() {
        let mut tokens = lex(
            "to <a href=\"/path?q=1&r=2#anchor\">there</a> now",
        );
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].0, "/path?q=1&r=2#anchor");
    }

    #[test]
    fn empty_href_still_produces_link() {
        // Empty href is "same document" per HTML; we don't second-
        // guess the markup. The renderer will emit an annotation
        // pointing at the empty URI — the malformed input shows up
        // in the PDF, not as crashes upstream.
        let mut tokens = lex("hit <a href=\"\">here</a> ok");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].0, "");
    }

    #[test]
    fn whitespace_around_attributes_is_tolerated() {
        let mut tokens =
            lex("hi <a   href = \"https://example.com\"   title  =  \"T\" >x</a>!");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].0, "https://example.com");
        assert_eq!(links[0].1.as_deref(), Some("T"));
    }

    #[test]
    fn close_only_anchor_left_alone() {
        // No opener — the </a> is orphaned and falls through.
        let mut tokens = lex("plain text </a> tail");
        rewrite_html_anchors(&mut tokens);
        assert!(collect_links(&tokens).is_empty());
    }

    #[test]
    fn two_adjacent_anchors() {
        let mut tokens = lex("<a href=\"u1\">a</a><a href=\"u2\">b</a>");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].0, "u1");
        assert_eq!(links[1].0, "u2");
    }

    #[test]
    fn unclosed_opener_does_not_eat_following_paragraph() {
        // Regression: an unclosed `<a href="…">` in one paragraph
        // followed by a `<a name="…">…</a>` in the next paragraph
        // used to scan past the OpenNoHref opener and pair with the
        // bookmark's `</a>`, swallowing the second paragraph into a
        // link. The OpenA-as-bail rule keeps them independent.
        let mut tokens = lex(
            "Unclosed: <a href=\"u\">no close here.\n\n\
             Bookmark: <a name=\"x\">target</a> done.\n",
        );
        rewrite_html_anchors(&mut tokens);
        assert!(
            collect_links(&tokens).is_empty(),
            "neither malformed anchor should be promoted to a Link: {tokens:?}"
        );
    }

    #[test]
    fn paragraph_break_inside_supposed_link_bails() {
        // A `<a href="…">` opener whose `</a>` only appears after a
        // blank line is malformed in spirit — bail and leave both
        // tags literal so the user sees what they wrote.
        let mut tokens =
            lex("Open <a href=\"u\">begin\n\nmiddle</a> end.\n");
        rewrite_html_anchors(&mut tokens);
        assert!(collect_links(&tokens).is_empty());
    }

    #[test]
    fn single_newline_inside_anchor_body_is_preserved() {
        // A single `\n` inside the supposed link body is a soft
        // line break within one paragraph — must NOT trigger the
        // paragraph-break bail.
        let mut tokens =
            lex("Open <a href=\"u\">begin\ncontinued</a> end.\n");
        rewrite_html_anchors(&mut tokens);
        let links = collect_links(&tokens);
        assert_eq!(links.len(), 1, "soft break should stay inside the link");
        assert_eq!(links[0].0, "u");
    }
}
