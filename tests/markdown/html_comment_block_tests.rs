//! Block-level HTML comments (CommonMark §4.6 type 2).
//!
//! Opener: `<!--` at line start (0–3 space indent allowed).
//! Body: runs to `-->` on this or a subsequent line.
//! Content: verbatim, including any text on the same line after `-->`.
//!
//! Distinct from the INLINE comment path tested in
//! `parse_html_comment_tests.rs`, which fires when `<!--` appears
//! mid-paragraph and produces `Token::HtmlComment` (carrying just the
//! comment body without delimiters).

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_html_block(tokens: &[Token]) -> Option<String> {
    tokens.iter().find_map(|t| match t {
        Token::HtmlBlock(s) => Some(s.clone()),
        _ => None,
    })
}

#[test]
fn single_line_comment() {
    let input = "<!-- hello -->\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn multi_line_comment_block() {
    let input = "<!--\nline one\nline two\n-->\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn comment_with_text_on_same_line_after_close_keeps_it_in_block() {
    // Spec example 177: `<!-- foo -->*bar*` — the `*bar*` after `-->`
    // is part of the block (verbatim, no emphasis), but the next line
    // starts fresh markdown parsing.
    let input = "<!-- foo -->*bar*\n*baz*\n";
    let tokens = parse(input);
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert_eq!(block, "<!-- foo -->*bar*\n");
    // *baz* on the following line IS an emphasis (paragraph).
    assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn comment_with_markdown_chars_inside_left_literal() {
    let input = "<!--\n*not emphasis*\n`not code`\n-->\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn comment_with_blank_lines_inside() {
    let input = "<!--\nfirst\n\nsecond\n-->\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn comment_with_one_space_indent() {
    let input = " <!-- foo -->\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with(" <!--"), "got {:?}", block);
}

#[test]
fn comment_with_three_space_indent() {
    let input = "   <!-- foo -->\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with("   <!--"), "got {:?}", block);
}

#[test]
fn four_space_indent_is_code_block_not_comment() {
    let tokens = parse("    <!-- foo -->\n");
    assert!(first_html_block(&tokens).is_none());
    assert!(tokens.iter().any(|t| matches!(t, Token::Code { block: true, .. })));
}

#[test]
fn comment_mid_paragraph_stays_inline() {
    // A `<!--` that doesn't start a line stays an inline HtmlComment.
    let tokens = parse("paragraph <!-- inline -->\n");
    assert!(first_html_block(&tokens).is_none());
    assert!(tokens.iter().any(|t| matches!(t, Token::HtmlComment(_))));
}

#[test]
fn comment_inside_blockquote() {
    let tokens = parse("> <!-- foo -->\n");
    let Some(Token::BlockQuote(body)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    assert!(
        body.iter().any(|t| matches!(t, Token::HtmlBlock(_))),
        "expected HtmlBlock inside BlockQuote, got {:?}",
        body
    );
}

#[test]
fn unterminated_comment_falls_through() {
    // No `-->` ever appears — the would-be block is rejected; falls
    // through to the inline path (which also rejects → literal text).
    let tokens = parse("<!-- never closes\nmore text\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn comment_with_partial_terminator_inside() {
    // A single `-` or `--` (without `>`) inside the body must NOT
    // close the block — full 3-char `-->` is required.
    let input = "<!-- has - and -- but no closer until -->\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn comment_followed_by_paragraph() {
    let input = "<!-- header -->\nbody paragraph\n";
    let tokens = parse(input);
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert_eq!(block, "<!-- header -->\n");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("body paragraph"), "got {:?}", text);
}
