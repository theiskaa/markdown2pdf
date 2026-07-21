//! Edge cases for `read_link_destination_and_title`, `read_title_delimited`,
//! `read_link_url_plain`. Existing modules cover happy-path links; this
//! module focuses on bracketed destinations, balanced parens, mismatched
//! quotes, and percent-encoding.

use markdown2pdf::markdown::*;

use super::common::parse;

fn link_parts(input: &str) -> (String, Option<String>) {
    let tokens = parse(input);
    let Some(Token::Link { url, title, .. }) =
        tokens.iter().find(|t| matches!(t, Token::Link { .. }))
    else {
        panic!("expected Link, got {:?}", tokens);
    };
    (url.clone(), title.clone())
}

#[test]
fn angle_bracket_destination_with_space() {
    let (url, _) = link_parts("[t](<a b>)");
    assert_eq!(url, "a b");
}

#[test]
fn angle_bracket_destination_with_escaped_close() {
    let (url, _) = link_parts(r"[t](<a\>b>)");
    assert_eq!(url, "a>b");
}

#[test]
fn balanced_parens_in_plain_url() {
    let (url, _) = link_parts("[Wiki](https://en.wikipedia.org/wiki/Foo(bar))");
    assert_eq!(url, "https://en.wikipedia.org/wiki/Foo(bar)");
}

#[test]
fn empty_plain_destination() {
    let (url, _) = link_parts("[t]()");
    assert_eq!(url, "");
}

#[test]
fn empty_bracketed_destination() {
    let (url, _) = link_parts("[t](<>)");
    assert_eq!(url, "");
}

#[test]
fn double_quoted_title_with_escaped_quote() {
    let (_, title) = link_parts(r#"[t](u "a\"b")"#);
    assert_eq!(title.as_deref(), Some("a\"b"));
}

#[test]
fn paren_title_with_escaped_close() {
    let (_, title) = link_parts(r"[t](u (a\)b))");
    assert_eq!(title.as_deref(), Some("a)b"));
}

#[test]
fn percent_encoded_chars_preserved() {
    let (url, _) = link_parts("[t](/path%20with%20space)");
    assert_eq!(url, "/path%20with%20space");
}

#[test]
fn backslash_escaped_punctuation_in_url() {
    let (url, _) = link_parts(r"[t](/a\(b\))");
    assert_eq!(url, "/a(b)");
}

#[test]
fn title_with_entity_decoded() {
    let (_, title) = link_parts(r#"[t](u "a &amp; b")"#);
    assert_eq!(title.as_deref(), Some("a & b"));
}
