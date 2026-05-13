//! ATX heading corner cases. Targets `is_atx_heading_start`,
//! `parse_heading`, `strip_atx_trailing_hashes`.

use markdown2pdf::markdown::*;

use super::common::parse;


fn heading_level_and_text(tokens: &[Token]) -> (usize, String) {
    let Some(Token::Heading(body, level)) =
        tokens.iter().find(|t| matches!(t, Token::Heading(_, _)))
    else {
        panic!("expected Heading, got {:?}", tokens);
    };
    (*level, Token::collect_all_text(body))
}

#[test]
fn one_to_six_hashes() {
    for n in 1..=6 {
        let input = format!("{} Heading\n", "#".repeat(n));
        let (level, text) = heading_level_and_text(&parse(&input));
        assert_eq!(level, n);
        assert_eq!(text.trim(), "Heading");
    }
}

#[test]
fn seven_hashes_is_not_a_heading() {
    let tokens = parse("####### too many\n");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Heading(_, _))));
}

#[test]
fn empty_heading() {
    let tokens = parse("#\n");
    let Some(Token::Heading(body, level)) = tokens.first() else {
        panic!("expected Heading, got {:?}", tokens);
    };
    assert_eq!(*level, 1);
    assert_eq!(Token::collect_all_text(body), "");
}

#[test]
fn trailing_hashes_stripped() {
    let (_, text) = heading_level_and_text(&parse("## heading ##\n"));
    assert_eq!(text.trim(), "heading");
}

#[test]
fn trailing_hashes_without_space_kept() {
    // `## heading##` — trailing hashes need preceding whitespace to be
    // recognized as a closing run.
    let (_, text) = heading_level_and_text(&parse("## heading##\n"));
    assert!(text.contains("heading##"));
}

#[test]
fn escaped_hash_does_not_start_heading() {
    let tokens = parse(r"\# not heading");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Heading(_, _))));
}

#[test]
fn one_to_three_space_indent_allowed() {
    for indent in 1..=3 {
        let input = format!("{}# Heading\n", " ".repeat(indent));
        assert!(parse(&input).iter().any(|t| matches!(t, Token::Heading(_, _))));
    }
}

#[test]
fn four_space_indent_is_code_block_not_heading() {
    let tokens = parse("    # not heading\n");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Heading(_, _))));
}
