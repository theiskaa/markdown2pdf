//! Block-element HTML blocks — opener is `<NAME` or `</NAME` where
//! NAME is one of the ~60 whitelisted block-level tags (`div`, `table`,
//! `p`, `blockquote`, `h1`–`h6`, `ul`, `ol`, `li`, `pre`-isn't-here-it's-
//! raw-content, etc.). Opener does NOT need to be syntactically
//! complete; body runs to the next blank line or EOF; content is
//! verbatim.
//!
//! Unlike the standalone-tag arm, this kind CAN interrupt an open
//! paragraph — a `<div>` immediately after non-blank prose terminates
//! the paragraph and opens a new block.

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_html_block(tokens: &[Token]) -> Option<String> {
    tokens.iter().find_map(|t| match t {
        Token::HtmlBlock(s) => Some(s.clone()),
        _ => None,
    })
}

fn html_blocks(tokens: &[Token]) -> Vec<String> {
    tokens
        .iter()
        .filter_map(|t| match t {
            Token::HtmlBlock(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn div_simple() {
    let input = "<div>\nbody\n</div>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn table_simple() {
    let input = "<table>\n<tr><td>x</td></tr>\n</table>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn paragraph_p_tag() {
    // `<p>` is in the whitelist — line-start `<p>` opens a block.
    let input = "<p>body</p>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn hr_tag() {
    // `<hr>` is whitelisted.
    let input = "<hr>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn heading_tags_h1_through_h6() {
    for n in 1..=6 {
        let input = format!("<h{}>title</h{}>\n", n, n);
        let tokens = parse(&input);
        assert_eq!(first_html_block(&tokens).as_deref(), Some(input.as_str()));
    }
}

#[test]
fn case_insensitive_tag_name() {
    let input = "<DIV CLASS=\"foo\">\nbody\n</DIV>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn incomplete_opener_still_triggers() {
    // Spec example 156: `<div id="foo"` (no closing `>`) is a valid
    // block start. Body runs to EOF.
    let input = "<div id=\"foo\"\n*hi*\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn opener_with_multiline_attributes() {
    // Spec example 153 shape.
    let input = "<div id=\"foo\"\n  class=\"bar\">\n</div>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn opener_with_attribute_value_split_across_lines() {
    // Spec example 154 shape.
    let input = "<div id=\"foo\" class=\"bar\n  baz\">\n</div>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn opener_with_invalid_attribute_chars_still_triggers() {
    // Spec example 158: `<div *???-&&&-<---` — Type 6 doesn't validate
    // tag syntax; only the tag name + delimiter matter.
    let input = "<div *???-&&&-<---\n*foo*\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn blank_line_terminates_block() {
    // Spec example 155 shape: blank line inside ends the block; what
    // follows is a separate paragraph (markdown re-enabled).
    let tokens = parse("<div>\n*foo*\n\n*bar*\n");
    let blocks = html_blocks(&tokens);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0], "<div>\n*foo*\n");
    // `*bar*` should be parsed as emphasis in the following paragraph.
    assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn blank_lines_inside_split_into_multiple_blocks_with_paragraphs_between() {
    // Spec example 188 / 152 shape.
    let tokens = parse("<div>\n\n*Emphasized*\n\n</div>\n");
    let blocks = html_blocks(&tokens);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0], "<div>\n");
    assert_eq!(blocks[1], "</div>\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn no_blank_lines_keeps_everything_inside_block() {
    // Spec example 189 / 186 shape: without blank lines markdown
    // inside stays literal.
    let input = "<div>\n*foo*\n</div>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn closer_then_text_outside_block() {
    // Spec example 186: `</div>\n*foo*` — the closer doesn't end the
    // block (only blank line does). `*foo*` stays inside the block.
    let input = "<div>\nbar\n</div>\n*foo*\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn can_interrupt_paragraph() {
    // Spec example 185: `Foo\n<div>...` — `<div>` opens a Type 6 block
    // even though the previous line is non-blank prose.
    let tokens = parse("Foo\n<div>\nbar\n</div>\n");
    let blocks = html_blocks(&tokens);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0], "<div>\nbar\n</div>\n");
    // `Foo` survives as text (the renderer wraps it in a paragraph).
    assert!(Token::collect_all_text(&tokens).contains("Foo"));
}

#[test]
fn block_with_one_space_indent() {
    let input = " <div>\nbody\n</div>\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with(" <div>"), "got {:?}", block);
}

#[test]
fn block_with_two_space_indent_then_body_blank_then_indented_code() {
    // Spec example 184 shape: 2-space indent on `<div>` is valid;
    // blank line ends the block; then 4-space-indent line becomes
    // an indented code block.
    let tokens = parse("  <div>\n\n    <div>\n");
    let blocks = html_blocks(&tokens);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0], "  <div>\n");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Code { block: true, .. }))
    );
}

#[test]
fn four_space_indent_opener_is_code_block() {
    // The 4-space-indented `<div>` line becomes an indented code block.
    // The subsequent `</div>` (no indent) at line start IS detected by
    // the Type 6 arm; both can coexist in one document.
    let tokens = parse("    <div>\nbody\n</div>\n");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Code { block: true, .. }))
    );
    // And the unindented closer line opens its own Type 6 block.
    let blocks = html_blocks(&tokens);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].contains("</div>"));
}

#[test]
fn block_inside_blockquote() {
    let tokens = parse("> <div>\n> body\n");
    let Some(Token::BlockQuote(body)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    assert!(body.iter().any(|t| matches!(t, Token::HtmlBlock(_))));
}

#[test]
fn many_back_to_back_blocks_with_blank_separators() {
    // Spec example 190 shape — each tag on its own line, blank lines
    // between, parses as many separate Type 6 blocks.
    let tokens = parse("<table>\n\n<tr>\n\n<td>\nHi\n</td>\n\n</tr>\n\n</table>\n");
    let blocks = html_blocks(&tokens);
    assert_eq!(blocks.len(), 5);
}

#[test]
fn complete_element_on_one_line() {
    // Spec example 159 shape: complete `<div><a href="bar">*foo*</a></div>`
    // on one line is one Type 6 block (markdown inside stays literal).
    let input = "<div><a href=\"bar\">*foo*</a></div>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn standalone_closer_opens_block() {
    // Spec example 151: `</div>` on its own line is a valid Type 6
    // start (closing tag, name in whitelist).
    let input = "</div>\n*foo*\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn non_whitelist_tag_does_not_open_block_here() {
    // `<a>` is NOT in the whitelist (it's phrasing) — falls through
    // to the standalone-tag arm. Verify we still get an HtmlBlock
    // (via that other arm).
    let tokens = parse("<a href=\"x\">\nbody\n</a>\n");
    assert!(first_html_block(&tokens).is_some());
}
