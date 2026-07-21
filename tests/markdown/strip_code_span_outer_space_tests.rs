use markdown2pdf::markdown::*;

use super::common::parse;

fn code_body(tokens: &[Token]) -> &str {
    match &tokens[0] {
        Token::Code { content, .. } => content,
        t => panic!("expected Code span, got {:?}", t),
    }
}

#[test]
fn both_sides_space_stripped() {
    let tokens = parse("` foo `");
    assert_eq!(code_body(&tokens), "foo");
}

#[test]
fn only_leading_space_preserved() {
    let tokens = parse("` foo`");
    assert_eq!(code_body(&tokens), " foo");
}

#[test]
fn only_trailing_space_preserved() {
    let tokens = parse("`foo `");
    assert_eq!(code_body(&tokens), "foo ");
}

#[test]
fn multiple_spaces_strip_one_each_side() {
    let tokens = parse("`  foo  `");
    assert_eq!(code_body(&tokens), " foo ");
}

#[test]
fn all_spaces_left_alone() {
    // Body is all spaces — strip rule explicitly bails.
    let tokens = parse("`   `");
    let body = code_body(&tokens);
    assert!(body.chars().all(|c| c == ' '));
    assert!(!body.is_empty());
}

#[test]
fn single_char_not_stripped() {
    let tokens = parse("`x`");
    assert_eq!(code_body(&tokens), "x");
}

#[test]
fn empty_body_left_alone() {
    let tokens = parse("``");
    // The lexer may render this as a code span with empty body or as
    // literal text — assert it doesn't crash and the code-span (if
    // produced) has an empty body.
    if let Some(Token::Code { content, .. }) = tokens.first() {
        assert_eq!(content, "");
    }
}
