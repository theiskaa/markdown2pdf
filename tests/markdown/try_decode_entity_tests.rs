//! Direct tests for `try_decode_entity` and `decode_escapes_and_entities`.
//! Covers named refs, numeric refs (dec/hex), surrogate/OOB → U+FFFD,
//! length caps, and entity decoding inside link URLs / titles.

use markdown2pdf::markdown::*;

use super::common::parse;


fn body(input: &str) -> String {
    Token::collect_all_text(&parse(input))
}

#[test]
fn named_amp() {
    assert!(body("&amp;").contains('&'));
}

#[test]
fn named_copy() {
    assert!(body("&copy;").contains('©'));
}

#[test]
fn named_long_entity() {
    // 31-char name — the longest valid CommonMark/WHATWG name.
    assert!(body("&CounterClockwiseContourIntegral;").contains('∳'));
}

#[test]
fn unknown_named_entity_kept_literal() {
    let text = body("&xyzzy;");
    assert!(text.contains("&xyzzy;"));
}

#[test]
fn missing_semicolon_kept_literal() {
    let text = body("&amp text");
    assert!(text.starts_with("&amp"));
}

#[test]
fn numeric_decimal() {
    assert!(body("&#65;").contains('A'));
}

#[test]
fn numeric_hex_lowercase_x() {
    assert!(body("&#x41;").contains('A'));
}

#[test]
fn numeric_hex_uppercase_x() {
    assert!(body("&#X41;").contains('A'));
}

#[test]
fn zero_codepoint_replaced_with_fffd() {
    assert!(body("&#0;").contains('\u{FFFD}'));
}

#[test]
fn surrogate_low_replaced_with_fffd() {
    assert!(body("&#xD800;").contains('\u{FFFD}'));
}

#[test]
fn surrogate_high_replaced_with_fffd() {
    assert!(body("&#xDFFF;").contains('\u{FFFD}'));
}

#[test]
fn over_max_codepoint_replaced_with_fffd() {
    assert!(body("&#x110000;").contains('\u{FFFD}'));
}

#[test]
fn empty_numeric_kept_literal() {
    let text = body("&#;");
    assert!(text.contains("&#;"));
}

#[test]
fn empty_hex_kept_literal() {
    let text = body("&#x;");
    assert!(text.contains("&#x;"));
}

#[test]
fn over_long_numeric_kept_literal() {
    // 8 decimal digits is > the 7-digit cap.
    let text = body("&#99999999;");
    assert!(text.contains("&#99999999;"));
}

#[test]
fn over_long_hex_kept_literal() {
    // 7 hex digits is > the 6-digit cap.
    let text = body("&#xABCDEF1;");
    assert!(text.contains("&#xABCDEF1;"));
}

#[test]
fn entity_inside_link_url_decoded() {
    let tokens = parse("[t](u&amp;v)");
    let Some(Token::Link { url, .. }) = tokens.first() else {
        panic!("expected Link, got {:?}", tokens);
    };
    assert_eq!(url, "u&v");
}

#[test]
fn entity_inside_link_title_decoded() {
    let tokens = parse(r#"[t](u "a&amp;b")"#);
    let Some(Token::Link { title, .. }) = tokens.first() else {
        panic!("expected Link, got {:?}", tokens);
    };
    assert_eq!(title.as_deref(), Some("a&b"));
}
