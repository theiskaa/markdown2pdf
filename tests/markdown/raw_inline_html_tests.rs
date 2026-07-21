use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn open_tag_inline() {
    let tokens = parse("text <span> more");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::HtmlInline(s) if s == "<span>")),
        "got {:?}",
        tokens
    );
}

#[test]
fn closing_tag_inline() {
    let tokens = parse("text </span> more");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::HtmlInline(s) if s == "</span>")),
        "got {:?}",
        tokens
    );
}

#[test]
fn open_tag_with_attribute() {
    // INLINE: needs a non-tag prefix on the same line — a complete tag
    // alone at line start is now a standalone-tag HtmlBlock. The
    // block-level path is tested in html_standalone_tag_block_tests.rs.
    let tokens = parse(r#"text <a href="https://example.com">"#);
    assert!(
        tokens.iter().any(|t| matches!(t, Token::HtmlInline(_))),
        "got {:?}",
        tokens
    );
}

#[test]
fn open_tag_self_closing() {
    let tokens = parse("text <br/>");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::HtmlInline(s) if s.contains("br"))),
        "got {:?}",
        tokens
    );
}

#[test]
fn html_comment_still_works() {
    // At line start, `<!--…-->` is now a block-level HtmlBlock per
    // CommonMark §4.6 type 2. The inline HtmlComment variant is
    // covered separately when the comment sits mid-paragraph.
    let tokens = parse("<!-- comment -->");
    assert!(matches!(tokens[0], Token::HtmlBlock(_)));
}

#[test]
fn autolink_still_works() {
    let tokens = parse("<https://example.com>");
    assert!(matches!(tokens[0], Token::Link { .. }));
}

#[test]
fn invalid_tag_falls_through_as_text() {
    let tokens = parse("<not a real tag>");
    let body = Token::collect_all_text(&tokens);
    assert!(body.contains("<not a real tag>"), "got {:?}", body);
}

#[test]
fn lt_alone_stays_text() {
    let tokens = parse("a < b is true");
    let body = Token::collect_all_text(&tokens);
    assert!(body.contains("<"), "got {:?}", body);
}

#[test]
fn surrounding_text_preserved() {
    let tokens = parse("before <em> middle </em> after");
    let body = Token::collect_all_text(&tokens);
    assert!(body.contains("before"), "got {:?}", body);
    assert!(body.contains("after"), "got {:?}", body);
    let html_count = tokens
        .iter()
        .filter(|t| matches!(t, Token::HtmlInline(_)))
        .count();
    assert_eq!(
        html_count, 2,
        "expected 2 HtmlInline tokens, got {:?}",
        tokens
    );
}
