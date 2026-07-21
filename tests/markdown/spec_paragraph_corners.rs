//! Paragraph interruption rules. A paragraph can be ended by a block
//! construct that interrupts it — these tests pin which constructs do
//! and don't.

use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn atx_heading_interrupts_paragraph() {
    let tokens = parse("paragraph\n# heading\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::Heading(_, 1))));
}

#[test]
fn fenced_code_interrupts_paragraph() {
    let tokens = parse("paragraph\n```\ncode\n```\n");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Code { block: true, .. }))
    );
}

#[test]
fn blockquote_interrupts_paragraph() {
    let tokens = parse("paragraph\n> quote\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::BlockQuote(_))));
}

#[test]
fn dashes_after_paragraph_make_setext_h2_not_hr() {
    // CommonMark precedence: `---` immediately after a paragraph forms a
    // setext heading (h2), not a thematic break. Pin that contract.
    let tokens = parse("paragraph\n---\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::Heading(_, 2))));
    assert!(!tokens.iter().any(|t| matches!(t, Token::HorizontalRule)));
}

#[test]
fn stars_after_paragraph_are_thematic_break() {
    // `***` is unambiguous — no setext form uses `*`, so it must interrupt
    // the paragraph as a thematic break.
    let tokens = parse("paragraph\n***\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::HorizontalRule)));
}

#[test]
fn list_starting_with_one_interrupts() {
    let tokens = parse("paragraph\n1. item\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::ListItem { .. })));
}

#[test]
fn bullet_list_interrupts() {
    let tokens = parse("paragraph\n- item\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::ListItem { .. })));
}
