//! HTML block Type 4 — declarations (CommonMark §4.6 type 4).
//!
//! Opener: `<!LETTER…` at line start (0–3 space indent allowed).
//! Body: runs to the first `>` on this or a subsequent line.
//! Content: verbatim, including the opening indent.

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_html_block(tokens: &[Token]) -> Option<String> {
    tokens.iter().find_map(|t| match t {
        Token::HtmlBlock(s) => Some(s.clone()),
        _ => None,
    })
}

#[test]
fn doctype_html() {
    let tokens = parse("<!DOCTYPE html>\n");
    assert_eq!(
        first_html_block(&tokens).as_deref(),
        Some("<!DOCTYPE html>\n"),
    );
}

#[test]
fn doctype_uppercase_keyword() {
    let tokens = parse("<!DOCTYPE HTML>\n");
    assert!(first_html_block(&tokens).is_some());
}

#[test]
fn element_declaration() {
    // CommonMark example 628 in INLINE context renders <!ELEMENT…> as
    // inline raw HTML; at line start it's a Type 4 block.
    let tokens = parse("<!ELEMENT br EMPTY>\n");
    assert_eq!(
        first_html_block(&tokens).as_deref(),
        Some("<!ELEMENT br EMPTY>\n"),
    );
}

#[test]
fn declaration_with_attlist() {
    let tokens = parse("<!ATTLIST foo bar CDATA #IMPLIED>\n");
    assert!(first_html_block(&tokens).is_some());
}

#[test]
fn declaration_with_one_space_indent() {
    // 0–3 space indent allowed before HTML block start.
    let tokens = parse(" <!DOCTYPE html>\n");
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    // Indent must be preserved verbatim in the block content.
    assert!(block.starts_with(" <!"), "got {:?}", block);
}

#[test]
fn declaration_with_three_space_indent() {
    let tokens = parse("   <!DOCTYPE html>\n");
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert!(block.starts_with("   <!"), "got {:?}", block);
}

#[test]
fn four_space_indent_is_code_block_not_html() {
    // 4-space indent → indented code block beats HTML block.
    let tokens = parse("    <!DOCTYPE html>\n");
    assert!(first_html_block(&tokens).is_none());
    assert!(tokens.iter().any(|t| matches!(t, Token::Code { block: true, .. })));
}

#[test]
fn declaration_not_at_line_start_is_inline_text() {
    // `<!DOCTYPE` mid-paragraph should NOT become a block — Types 1-7
    // require line-start position.
    let tokens = parse("paragraph <!DOCTYPE html>\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn declaration_inside_blockquote() {
    // Blockquote sub-lexer strips `> ` then re-parses, so the inner
    // `<!DOCTYPE>` should produce an HtmlBlock inside the quote.
    let tokens = parse("> <!DOCTYPE html>\n");
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
fn opener_without_letter_is_not_a_declaration() {
    // `<!` followed by non-letter is not a Type 4 opener. The existing
    // inline `<!--…-->` path may take over for `<!--`, but anything
    // else should not be claimed as a block declaration.
    let tokens = parse("<!?> not a declaration\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn unterminated_declaration_falls_through() {
    // No `>` ever appears — the would-be block is rejected so we
    // don't consume the rest of the document. Falls through to inline
    // tag matching (which also rejects, so it becomes paragraph text).
    let tokens = parse("<!DOCTYPE unterminated\nmore text\n");
    assert!(first_html_block(&tokens).is_none());
}
