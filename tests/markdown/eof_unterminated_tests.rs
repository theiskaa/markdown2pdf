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

/// Invariant: the lexer never bakes a literal `\n` into a `Token::Text`.
/// A line break must always survive as `Token::Newline` so lowering can
/// turn it into a space — a raw `\n` inside a text run reaches the
/// renderer, which has no glyph for it and paints a missing-glyph box
/// on the embedded-font path. Regression for the `parse_link`
/// no-closing-bracket fallback, which used to flatten multi-line bodies
/// into one `Text` with embedded newlines.
fn assert_no_literal_newline_in_text(input: &str) {
    fn walk(t: &Token, input: &str) {
        match t {
            Token::Text(s) => assert!(
                !s.contains('\n'),
                "Token::Text carries a literal newline for {input:?}: {s:?}"
            ),
            Token::Heading(inner, _)
            | Token::Emphasis { content: inner, .. }
            | Token::StrongEmphasis(inner)
            | Token::Strikethrough(inner)
            | Token::Highlight(inner)
            | Token::BlockQuote(inner)
            | Token::ListItem { content: inner, .. }
            | Token::Link { content: inner, .. }
            | Token::Image { alt: inner, .. }
            | Token::FootnoteDefinition { content: inner, .. }
            | Token::InlineFootnote { content: inner, .. } => {
                for c in inner {
                    walk(c, input);
                }
            }
            _ => {}
        }
    }
    for t in &parse(input) {
        walk(t, input);
    }
}

#[test]
fn unclosed_bracket_spanning_newline_keeps_newline_token() {
    // Plain failed link across a soft break.
    assert_no_literal_newline_in_text("[never closed and stuff\nmore text\n");
    // Degraded inline footnote (`^[`) across a soft break + EOF newline.
    assert_no_literal_newline_in_text(
        "a stray ^[never closed and an empty ^[] both\nrender as text done.\n",
    );
    // The break must actually be preserved as a Newline, not dropped.
    let toks = parse("see [a b c\nd e f\n");
    assert!(
        toks.iter().any(|t| matches!(t, Token::Newline)),
        "soft break inside an unclosed link was lost entirely: {toks:?}"
    );
}
