//! Standalone HTML tag blocks — a complete open or close tag on its
//! own line whose tag name is NOT in the raw-content set
//! (`script/pre/style/textarea`) and NOT in the block-element whitelist
//! (`div/table/p/…`).
//!
//! Opener: `<tag>` or `</tag>` at line start (0–3 space indent allowed),
//! the tag must fit on a single line, and only spaces/tabs may follow
//! before end-of-line.
//! Body: runs until the next blank line or EOF; content is verbatim.
//!
//! Critically, this kind CANNOT interrupt an open paragraph — a line
//! like `<a href="x">` immediately after non-blank prose stays as
//! paragraph continuation, not a new block.

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_html_block(tokens: &[Token]) -> Option<String> {
    tokens.iter().find_map(|t| match t {
        Token::HtmlBlock(s) => Some(s.clone()),
        _ => None,
    })
}

#[test]
fn anchor_block_with_body() {
    let input = "<a href=\"foo\">\n*bar*\n</a>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
    // Body markdown chars stay literal — no emphasis emitted.
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn italic_phrasing_tag_block() {
    let input = "<i class=\"foo\">\n*bar*\n</i>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn del_phrasing_tag_block() {
    let input = "<del>\n*foo*\n</del>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn custom_tag_block() {
    // A custom-element tag name not in any whitelist.
    let input = "<Warning>\n*bar*\n</Warning>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn closing_tag_alone_opens_block() {
    // `</ins>` at line start opens its own standalone-tag block.
    let input = "</ins>\n*bar*\n";
    let tokens = parse(input);
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert!(block.starts_with("</ins>"), "got {:?}", block);
}

#[test]
fn blank_line_separates_two_standalone_blocks() {
    // `<del>` then blank line then markdown then blank then `</del>`:
    // becomes two separate HTML blocks plus a paragraph between them.
    let input = "<del>\n\n*foo*\n\n</del>\n";
    let tokens = parse(input);
    let blocks: Vec<&String> = tokens
        .iter()
        .filter_map(|t| match t {
            Token::HtmlBlock(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(blocks.len(), 2, "expected 2 HtmlBlocks, got {:?}", blocks);
    assert!(blocks[0].contains("<del>"));
    assert!(blocks[1].contains("</del>"));
    // The middle paragraph has an emphasis.
    assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn tag_followed_by_non_whitespace_does_not_open_block() {
    // `<a>x` on a line — after the complete tag there's `x` (not
    // whitespace), so the line isn't a standalone-tag block.
    let tokens = parse("<a>x\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn cannot_interrupt_open_paragraph() {
    // `Foo` on line 1 starts a paragraph; `<a href="x">` on line 2
    // looks like it could open a block, but the precedence rule says
    // standalone tags can't interrupt paragraphs.
    let tokens = parse("Foo\n<a href=\"x\">\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn does_open_after_blank_line() {
    // Same tag, but with a blank line before it — now it opens a block.
    let tokens = parse("Foo\n\n<a href=\"x\">\n");
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert!(block.starts_with("<a href"), "got {:?}", block);
}

#[test]
fn multi_line_tag_does_not_open_block() {
    // Tag attribute value spans onto a continuation line — the tag
    // itself isn't on a single line, so per spec it's NOT a Type 7
    // block. Stays as inline HTML inside a paragraph (or text).
    let tokens = parse("<a href=\"foo  \nbar\">\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn block_element_tag_name_goes_to_block_element_arm() {
    // `<a>` is fine for standalone-tag; `<div>` is NOT — it's claimed
    // by the block-element arm instead. Both arms produce HtmlBlock,
    // but their precedence + opener rules differ (block-element can
    // interrupt a paragraph; opener can be incomplete).
    let tokens = parse("<div>\n*foo*\n</div>\n");
    // Block IS produced (by the block-element arm).
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert!(block.contains("<div>"));
    // Markdown chars inside stay literal — no emphasis emitted.
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn raw_content_tag_name_does_not_open_block_here() {
    // `<script>` is reserved for the raw-content arm (handled by an
    // earlier check) — the standalone-tag arm shouldn't claim it.
    // We still get an HtmlBlock here, but from the raw-content arm —
    // verify by checking the block contains `</script>`.
    let tokens = parse("<script>code</script>\n");
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert!(block.contains("</script>"));
}

#[test]
fn block_with_one_space_indent() {
    // The opener line must be followed only by whitespace, so use a
    // tag that's alone on its line.
    let input = " <a href=\"x\">\nbody\n</a>\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with(" <a "), "got {:?}", block);
}

#[test]
fn block_with_three_space_indent() {
    let input = "   <a href=\"x\">\nbody\n</a>\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with("   <a "), "got {:?}", block);
}

#[test]
fn four_space_indent_is_code_block_not_html() {
    let tokens = parse("    <a href=\"x\">\nbody\n</a>\n");
    assert!(first_html_block(&tokens).is_none());
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Code { block: true, .. }))
    );
}

#[test]
fn block_inside_blockquote() {
    let tokens = parse("> <a href=\"x\">\n");
    let Some(Token::BlockQuote(body)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    assert!(body.iter().any(|t| matches!(t, Token::HtmlBlock(_))));
}

#[test]
fn self_closing_tag_opens_block() {
    let input = "<br/>\n";
    let tokens = parse(input);
    assert!(first_html_block(&tokens).is_some());
}

#[test]
fn malformed_tag_does_not_open_block() {
    // Tag with invalid attribute syntax — `try_match_html_tag_len`
    // returns None, so the standalone-tag arm doesn't fire and the
    // line falls through to inline / text handling.
    let tokens = parse("<a *!@#>\n");
    assert!(first_html_block(&tokens).is_none());
}
