//! Setext heading corner cases. Targets `peek_setext_level`,
//! `consume_setext_heading`.

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_heading_level(tokens: &[Token]) -> usize {
    let Some(Token::Heading(_, level)) = tokens.iter().find(|t| matches!(t, Token::Heading(_, _)))
    else {
        panic!("expected Heading, got {:?}", tokens);
    };
    *level
}

#[test]
fn equals_makes_level_one() {
    assert_eq!(first_heading_level(&parse("Title\n=====\n")), 1);
}

#[test]
fn dashes_make_level_two() {
    assert_eq!(first_heading_level(&parse("Title\n-----\n")), 2);
}

#[test]
fn single_equals_works() {
    // A single `=` is enough for setext.
    assert_eq!(first_heading_level(&parse("Title\n=\n")), 1);
}

#[test]
fn single_dash_works() {
    assert_eq!(first_heading_level(&parse("Title\n-\n")), 2);
}

#[test]
fn setext_with_multi_line_paragraph_above() {
    // All preceding paragraph lines become the heading content.
    let tokens = parse("first\nsecond\n===\n");
    let Some(Token::Heading(body, _)) = tokens.first() else {
        panic!("expected Heading, got {:?}", tokens);
    };
    let text = Token::collect_all_text(body);
    assert!(text.contains("first"));
    assert!(text.contains("second"));
}

#[test]
fn blank_line_breaks_setext() {
    // Blank between text and underline disqualifies the underline.
    let tokens = parse("Title\n\n===\n");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Heading(_, _))));
}

#[test]
fn dash_can_be_thematic_break_instead() {
    // Single `-` after blank line is paragraph (no para to attach to) → not HR;
    // but `---` after blank is HR. Pin: `---` alone is HR, not setext.
    let tokens = parse("\n---\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::HorizontalRule)));
}

#[test]
fn setext_underline_length_independent_of_content() {
    // Underline length doesn't need to match the title.
    assert_eq!(first_heading_level(&parse("A very long heading\n=\n")), 1);
}
