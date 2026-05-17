//! WikiLink lexing: `[[Target]]` / `[[Target|Label]]` become a
//! `Token::Link` whose URL is `#<slug-of-target>` (the same shape an
//! explicit `[text](#slug)` cross-reference produces) and whose
//! content is the label, or the target when no label is given.
//! Degenerate forms (unclosed, multi-line, empty/symbol-only target,
//! escaped) are NOT wikilinks and stay literal text.

use markdown2pdf::markdown::*;

use super::common::parse;

/// Collect every `Token::Link` (url, flattened text) reachable in the
/// token tree, so a wikilink can be asserted regardless of the inline
/// context (heading, list, blockquote, emphasis, table) it sits in.
fn links(tokens: &[Token]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    fn walk(ts: &[Token], out: &mut Vec<(String, String)>) {
        for t in ts {
            match t {
                Token::Link { content, url, .. } => {
                    out.push((url.clone(), Token::collect_all_text(content)));
                    walk(content, out);
                }
                Token::Heading(c, _)
                | Token::StrongEmphasis(c)
                | Token::BlockQuote(c)
                | Token::Strikethrough(c) => walk(c, out),
                Token::Emphasis { content, .. } | Token::ListItem { content, .. } => {
                    walk(content, out)
                }
                Token::Table { headers, rows, .. } => {
                    for cell in headers {
                        walk(cell, out);
                    }
                    for row in rows {
                        for cell in row {
                            walk(cell, out);
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

fn has_link(tokens: &[Token]) -> bool {
    !links(tokens).is_empty()
}

#[test]
fn bare_target_links_to_its_slug() {
    let tokens = parse("[[Introduction]]");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("Introduction".to_string())],
            url: "#introduction".to_string(),
            title: None,
        }]
    );
}

#[test]
fn spaced_target_slugifies() {
    assert_eq!(links(&parse("[[Page Name]]")), vec![("#page-name".to_string(), "Page Name".to_string())]);
}

#[test]
fn pipe_label_is_the_visible_text() {
    assert_eq!(
        parse("[[introduction|see the intro]]"),
        vec![Token::Link {
            content: vec![Token::Text("see the intro".to_string())],
            url: "#introduction".to_string(),
            title: None,
        }]
    );
}

#[test]
fn target_whitespace_is_trimmed() {
    assert_eq!(
        links(&parse("[[  Introduction  ]]")),
        vec![("#introduction".to_string(), "Introduction".to_string())]
    );
}

#[test]
fn only_the_first_pipe_splits_target_from_label() {
    assert_eq!(
        links(&parse("[[Topic|first|second]]")),
        vec![("#topic".to_string(), "first|second".to_string())]
    );
}

#[test]
fn trailing_bracket_after_close_is_literal() {
    let tokens = parse("[[X]]]");
    assert_eq!(links(&tokens), vec![("#x".to_string(), "X".to_string())]);
    assert!(Token::collect_all_text(&tokens).contains(']'));
}

#[test]
fn back_to_back_wikilinks_are_two_links() {
    assert_eq!(
        links(&parse("[[Alpha]][[Beta]]")),
        vec![
            ("#alpha".to_string(), "Alpha".to_string()),
            ("#beta".to_string(), "Beta".to_string()),
        ]
    );
}

#[test]
fn wikilink_inside_emphasis() {
    assert_eq!(links(&parse("*[[X]]*")), vec![("#x".to_string(), "X".to_string())]);
}

#[test]
fn wikilink_inside_list_item() {
    assert_eq!(links(&parse("- see [[X]]")), vec![("#x".to_string(), "X".to_string())]);
}

#[test]
fn wikilink_inside_blockquote() {
    assert_eq!(links(&parse("> jump to [[X]]")), vec![("#x".to_string(), "X".to_string())]);
}

#[test]
fn wikilink_inside_heading() {
    assert_eq!(links(&parse("# A [[X]] heading")), vec![("#x".to_string(), "X".to_string())]);
}

#[test]
fn wikilink_inside_table_cell() {
    let md = "| H |\n| - |\n| [[X]] |\n";
    assert_eq!(links(&parse(md)), vec![("#x".to_string(), "X".to_string())]);
}

#[test]
fn unclosed_wikilink_is_literal_text() {
    let tokens = parse("[[Unclosed and more text");
    assert!(!has_link(&tokens));
    assert!(Token::collect_all_text(&tokens).contains("[[Unclosed"));
}

#[test]
fn single_closing_bracket_is_not_a_wikilink() {
    let tokens = parse("[[Half]");
    assert!(!has_link(&tokens));
}

#[test]
fn newline_inside_is_not_a_wikilink() {
    let tokens = parse("[[multi\nline]]");
    assert!(!has_link(&tokens));
    assert!(Token::collect_all_text(&tokens).contains("[[multi"));
}

#[test]
fn empty_and_symbol_only_targets_degrade_to_text() {
    for src in ["[[]]", "[[ ]]", "[[|just a label]]", "[[$$$]]"] {
        let tokens = parse(src);
        assert!(!has_link(&tokens), "{src:?} must not be a wikilink");
        assert!(
            Token::collect_all_text(&tokens).contains("[["),
            "{src:?} must keep its literal brackets"
        );
    }
}

#[test]
fn escaped_brackets_render_literally() {
    let tokens = parse(r"\[\[Not a wikilink\]\]");
    assert!(!has_link(&tokens));
    assert_eq!(
        Token::collect_all_text(&tokens),
        "[[Not a wikilink]]".to_string()
    );
}

/// Locks the lexer's target→slug to the renderer's heading→slug rule
/// (both call the shared `slugify`): mixed case, spaces, underscores
/// and punctuation must collapse the same way a heading of that text
/// would, or the link would never resolve.
#[test]
fn target_slug_matches_heading_slug_rule() {
    assert_eq!(
        links(&parse("[[Foo Bar_Baz (v2)!]]")),
        vec![("#foo-bar-baz-v2".to_string(), "Foo Bar_Baz (v2)!".to_string())]
    );
}
