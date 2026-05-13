//! HTML block Type 5 — CDATA sections (CommonMark §4.6 type 5).
//!
//! Opener: `<![CDATA[` at line start (0–3 space indent allowed).
//! Body: runs to `]]>` on this or a subsequent line.
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
fn cdata_single_line() {
    let tokens = parse("<![CDATA[foo]]>\n");
    assert_eq!(
        first_html_block(&tokens).as_deref(),
        Some("<![CDATA[foo]]>\n"),
    );
}

#[test]
fn cdata_multi_line() {
    let input = "<![CDATA[\nline one\nline two\n]]>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn cdata_with_markdown_chars_left_literal() {
    // Markdown-meaningful characters inside CDATA stay literal — no
    // emphasis or code-span parsing happens within an HTML block.
    let input = "<![CDATA[\n*not emphasis*\n`not code`\n]]>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
    // And nothing emphasized leaked out.
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn cdata_with_angle_brackets_and_ampersands_inside() {
    // The whole point of CDATA — `<`, `>`, `&` are literal data.
    let input = "<![CDATA[a < b && c > d]]>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn cdata_with_blank_lines_inside() {
    // Spec example 182 shape — CDATA body can contain blank lines.
    let input = "<![CDATA[\nfirst\n\nsecond\n]]>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn cdata_followed_by_paragraph() {
    // Spec example 182: paragraph follows the closing `]]>` line.
    let input = "<![CDATA[\nbody\n]]>\nokay\n";
    let tokens = parse(input);
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert_eq!(block, "<![CDATA[\nbody\n]]>\n");
    // `okay` should still appear in the token stream as paragraph text.
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("okay"), "got {:?}", text);
}

#[test]
fn cdata_with_one_space_indent() {
    let input = " <![CDATA[foo]]>\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with(" <!["), "got {:?}", block);
}

#[test]
fn cdata_with_three_space_indent() {
    let input = "   <![CDATA[foo]]>\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with("   <!["), "got {:?}", block);
}

#[test]
fn four_space_indent_is_code_block_not_cdata() {
    let tokens = parse("    <![CDATA[foo]]>\n");
    assert!(first_html_block(&tokens).is_none());
    assert!(tokens.iter().any(|t| matches!(t, Token::Code { block: true, .. })));
}

#[test]
fn cdata_not_at_line_start_is_inline_text() {
    // CDATA mid-paragraph is not a block — handled by inline raw-HTML
    // (which today falls through to text; a later commit adds inline
    // CDATA support per CommonMark spec example 629).
    let tokens = parse("paragraph <![CDATA[foo]]>\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn cdata_inside_blockquote() {
    let tokens = parse("> <![CDATA[foo]]>\n");
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
fn unterminated_cdata_falls_through() {
    // No `]]>` ever appears — the would-be block is rejected so the
    // remainder of the document isn't swallowed. The opener falls
    // through to inline text.
    let tokens = parse("<![CDATA[never closes\nmore text\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn cdata_with_partial_terminator_inside() {
    // A `]` or `]]` (without `>`) inside the body should NOT close
    // the block — the full 3-char terminator is required.
    let input = "<![CDATA[contains ]] and ] but no closer until]]>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}
