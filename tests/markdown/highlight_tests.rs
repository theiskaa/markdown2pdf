//! Inline highlight lexing: `==text==` becomes `Token::Highlight`,
//! nestable with other inline styles. A `==`/`===` line that
//! underlines a paragraph stays a Setext heading (dispatch runs after
//! Setext detection); an unterminated `==` degrades to literal text.

use markdown2pdf::markdown::*;

use super::common::parse;

/// Every `Token::Highlight`'s flattened text, in document order,
/// regardless of the inline context it is nested in.
fn highlights(tokens: &[Token]) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(ts: &[Token], out: &mut Vec<String>) {
        for t in ts {
            match t {
                Token::Highlight(c) => {
                    out.push(Token::collect_all_text(c));
                    walk(c, out);
                }
                Token::Heading(c, _)
                | Token::StrongEmphasis(c)
                | Token::BlockQuote(c)
                | Token::Strikethrough(c) => walk(c, out),
                Token::Emphasis { content, .. }
                | Token::ListItem { content, .. }
                | Token::Link { content, .. } => walk(content, out),
                Token::Table { headers, rows, .. } => {
                    for cell in headers {
                        walk(&cell.content, out);
                    }
                    for row in rows {
                        for cell in row {
                            walk(&cell.content, out);
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

fn has_highlight(tokens: &[Token]) -> bool {
    !highlights(tokens).is_empty()
}

#[test]
fn highlights_only_the_marked_span() {
    let tokens = parse("Some ==important== text");
    assert_eq!(highlights(&tokens), vec!["important".to_string()]);
    assert_eq!(
        Token::collect_all_text(&tokens),
        "Some important text".to_string()
    );
    assert!(matches!(tokens[0], Token::Text(ref s) if s.starts_with("Some")));
}

#[test]
fn bare_highlight() {
    assert_eq!(
        parse("==hi=="),
        vec![Token::Highlight(vec![Token::Text("hi".to_string())])]
    );
}

#[test]
fn nested_bold_is_bold_and_highlighted() {
    let tokens = parse("==**bold**==");
    assert_eq!(
        tokens,
        vec![Token::Highlight(vec![Token::Emphasis {
            level: 2,
            content: vec![Token::Text("bold".to_string())],
        }])]
    );
}

#[test]
fn nested_emphasis_inside_highlight() {
    let tokens = parse("==a *b* c==");
    assert_eq!(highlights(&tokens), vec!["a b c".to_string()]);
}

#[test]
fn highlight_inside_emphasis() {
    assert_eq!(highlights(&parse("*see ==this==*")), vec!["this".to_string()]);
}

#[test]
fn highlight_inside_list_blockquote_heading_table() {
    assert_eq!(highlights(&parse("- ==a==")), vec!["a".to_string()]);
    assert_eq!(highlights(&parse("> ==b==")), vec!["b".to_string()]);
    assert_eq!(highlights(&parse("# ==c==")), vec!["c".to_string()]);
    assert_eq!(
        highlights(&parse("| H |\n| - |\n| ==d== |\n")),
        vec!["d".to_string()]
    );
}

#[test]
fn setext_h1_underline_is_not_a_highlight() {
    let tokens = parse("Foo\n===\n");
    assert!(!has_highlight(&tokens));
    assert_eq!(
        tokens,
        vec![Token::Heading(vec![Token::Text("Foo".to_string())], 1)]
    );
}

#[test]
fn setext_h2_underline_still_works() {
    let tokens = parse("Bar\n---\n");
    assert!(!has_highlight(&tokens));
    assert_eq!(
        tokens,
        vec![Token::Heading(vec![Token::Text("Bar".to_string())], 2)]
    );
}

#[test]
fn standalone_equals_line_degrades_to_text() {
    let tokens = parse("===\n");
    assert!(!has_highlight(&tokens));
    assert!(Token::collect_all_text(&tokens).contains("==="));
}

#[test]
fn unterminated_highlight_degrades_to_text() {
    let tokens = parse("Some ==important but no closer");
    assert!(!has_highlight(&tokens));
    assert!(Token::collect_all_text(&tokens).contains("=="));
}

#[test]
fn escaped_equals_are_literal() {
    let tokens = parse(r"\=\=not a highlight\=\=");
    assert!(!has_highlight(&tokens));
    assert_eq!(
        Token::collect_all_text(&tokens),
        "==not a highlight==".to_string()
    );
}

#[test]
fn single_equals_is_literal_text() {
    let tokens = parse("a = b = c");
    assert!(!has_highlight(&tokens));
    assert_eq!(Token::collect_all_text(&tokens), "a = b = c".to_string());
}

#[test]
fn back_to_back_highlights() {
    assert_eq!(
        highlights(&parse("==one====two==")),
        vec!["one".to_string(), "two".to_string()]
    );
}
