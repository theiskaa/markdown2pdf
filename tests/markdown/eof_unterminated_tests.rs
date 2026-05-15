//! Constructs left unterminated at end-of-input must degrade to a
//! sane token (literal text / a best-effort node), never panic or
//! bubble a lexer error. `parse` unwraps, so a `LexerError` here is
//! a test failure by construction.

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_is_text(input: &str) {
    let toks = parse(input);
    assert!(!toks.is_empty(), "no tokens for {input:?}");
    assert!(
        matches!(toks[0], Token::Text(_)),
        "expected leading Text for {input:?}, got {toks:?}"
    );
}

#[test]
fn unterminated_cdata_at_eof() {
    let toks = parse("<![CDATA[ never closed");
    assert!(!toks.is_empty());
}

#[test]
fn unterminated_processing_instruction_at_eof() {
    first_is_text("<?php echo 1 never closed");
}

#[test]
fn unterminated_doctype_at_eof() {
    first_is_text("<!DOCTYPE html never closed");
}

#[test]
fn unterminated_autolink_at_eof() {
    first_is_text("<http://example.com/no-close");
}

#[test]
fn unterminated_link_title_at_eof() {
    let toks = parse("[a](http://e \"unterminated title");
    assert!(!toks.is_empty());
}

#[test]
fn footnote_definition_at_eof() {
    // A footnote definition with no following content still parses.
    let toks = parse("[^1]: a dangling note with no newline");
    assert!(matches!(
        toks[0],
        Token::FootnoteDefinition { .. } | Token::Text(_)
    ));
}

#[test]
fn table_header_row_only_at_eof() {
    // A header row with no delimiter row is not a table — it stays
    // paragraph text.
    first_is_text("| a | b | c |");
}

#[test]
fn unterminated_html_comment_at_eof() {
    let toks = parse("<!-- never closed");
    assert!(!toks.is_empty());
}

#[test]
fn unterminated_fenced_code_at_eof() {
    let toks = parse("```rust\nfn main() {}");
    assert!(!toks.is_empty());
}
