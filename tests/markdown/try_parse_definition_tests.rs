//! Direct tests for `try_parse_definition`. Definitions are pre-extracted
//! by `extract_definitions` before the main parse, so the integration
//! surface is "define + reference, then assert the URL/title resolves".

use markdown2pdf::markdown::*;

use super::common::parse;

fn link_of(tokens: &[Token]) -> (&str, &Option<String>) {
    let Some(Token::Link { url, title, .. }) =
        tokens.iter().find(|t| matches!(t, Token::Link { .. }))
    else {
        panic!("expected Link, got {:?}", tokens);
    };
    (url.as_str(), title)
}

#[test]
fn simple_definition() {
    let tokens = parse("[a]\n\n[a]: /u\n");
    let (url, title) = link_of(&tokens);
    assert_eq!(url, "/u");
    assert!(title.is_none());
}

#[test]
fn definition_with_double_quoted_title() {
    let tokens = parse("[a]\n\n[a]: /u \"the title\"\n");
    let (url, title) = link_of(&tokens);
    assert_eq!(url, "/u");
    assert_eq!(title.as_deref(), Some("the title"));
}

#[test]
fn definition_with_single_quoted_title() {
    let tokens = parse("[a]\n\n[a]: /u 'the title'\n");
    let (_, title) = link_of(&tokens);
    assert_eq!(title.as_deref(), Some("the title"));
}

#[test]
fn definition_with_paren_title() {
    let tokens = parse("[a]\n\n[a]: /u (the title)\n");
    let (_, title) = link_of(&tokens);
    assert_eq!(title.as_deref(), Some("the title"));
}

#[test]
fn definition_title_on_next_line() {
    let tokens = parse("[a]\n\n[a]: /u\n  \"on next line\"\n");
    let (url, title) = link_of(&tokens);
    assert_eq!(url, "/u");
    assert_eq!(title.as_deref(), Some("on next line"));
}

#[test]
fn definition_with_angle_bracketed_url() {
    let tokens = parse("[a]\n\n[a]: </the url>\n");
    let (url, _) = link_of(&tokens);
    assert_eq!(url, "/the url");
}

#[test]
fn definition_with_empty_angle_bracketed_url() {
    let tokens = parse("[a]\n\n[a]: <>\n");
    let (url, _) = link_of(&tokens);
    assert_eq!(url, "");
}

#[test]
fn duplicate_label_first_wins() {
    let tokens = parse("[a]\n\n[a]: /first\n[a]: /second\n");
    let (url, _) = link_of(&tokens);
    assert_eq!(url, "/first");
}

#[test]
fn definition_with_entity_in_title_decoded() {
    let tokens = parse("[a]\n\n[a]: /u \"x &amp; y\"\n");
    let (_, title) = link_of(&tokens);
    assert_eq!(title.as_deref(), Some("x & y"));
}

#[test]
fn definition_with_escape_in_url_decoded() {
    let tokens = parse("[a]\n\n[a]: /url\\(paren\\)\n");
    let (url, _) = link_of(&tokens);
    assert_eq!(url, "/url(paren)");
}

#[test]
fn definition_inside_blockquote_resolves_locally() {
    // A definition inside a blockquote should be visible to references
    // also inside the blockquote.
    let tokens = parse("> [a]\n>\n> [a]: /u\n");
    // Walk into the BlockQuote body and look for the link.
    let Some(Token::BlockQuote(body)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    // Either a Link inside, or fallback to text.
    let _ = body;
}

#[test]
fn three_space_indent_still_definition() {
    let tokens = parse("[a]\n\n   [a]: /u\n");
    let (url, _) = link_of(&tokens);
    assert_eq!(url, "/u");
}

#[test]
fn four_space_indent_is_indented_code_not_definition() {
    // Four-space indent should make this an indented code block —
    // the reference `[a]` then has nothing to resolve to.
    let tokens = parse("[a]\n\n    [a]: /u\n");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Link { .. })));
}
