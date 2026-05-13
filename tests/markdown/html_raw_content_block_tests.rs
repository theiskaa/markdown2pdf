//! Raw-content HTML blocks — `<script>`, `<pre>`, `<style>`, `<textarea>`.
//!
//! Opener: one of those tag names (case-insensitive) at line start,
//! followed by a space, tab, `>`, or end-of-line. Up to 3 spaces of
//! indent allowed.
//! Body: runs to the first line that contains any of the four closing
//! tags (`</script>`, `</pre>`, `</style>`, `</textarea>`,
//! case-insensitive), or to EOF if no closer ever appears.
//! Content: verbatim — no markdown parsing inside.

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_html_block(tokens: &[Token]) -> Option<String> {
    tokens.iter().find_map(|t| match t {
        Token::HtmlBlock(s) => Some(s.clone()),
        _ => None,
    })
}

#[test]
fn script_block_basic() {
    let input = "<script>foo</script>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn pre_block_basic() {
    let input = "<pre>foo</pre>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn style_block_basic() {
    let input = "<style>p{color:red;}</style>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn textarea_block_basic() {
    let input = "<textarea>type here</textarea>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn opener_with_attributes() {
    let input = "<script type=\"text/javascript\">\ncode\n</script>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn opener_with_multiline_attributes() {
    // Spec example 172 shape: attributes split across lines.
    let input = "<style\n  type=\"text/css\">\nh1 {color:red;}\n</style>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn body_with_blank_lines() {
    // Spec example 171 shape: blank lines inside body don't end the block.
    let input = "<textarea>\n\n*foo*\n\n_bar_\n\n</textarea>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
    // Markdown chars inside stay literal.
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn body_with_markdown_chars_left_literal() {
    let input = "<script>\n*not emphasis*\n`not code`\n[not](link)\n</script>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    assert!(!tokens.iter().any(|t| matches!(t, Token::Link { .. })));
}

#[test]
fn case_insensitive_opener() {
    let input = "<SCRIPT>code</SCRIPT>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn case_insensitive_closer() {
    // Opener lowercase, closer uppercase — both must match.
    let input = "<script>code</SCRIPT>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn closer_with_content_after_on_same_line() {
    // Spec example 178: `</script>1. *bar*` — the whole line is part
    // of the block, including the markdown-looking text after `</script>`.
    let input = "<script>\nfoo\n</script>1. *bar*\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn closer_need_not_match_opener_tag() {
    // CommonMark §4.6 explicitly says the closer "need not match the
    // start tag" — `<script>` can be closed by `</pre>` and vice versa.
    let input = "<script>code</pre>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn unclosed_runs_to_eof() {
    // Spec example 173: no closer ever appears — block consumes the
    // rest of the document.
    let input = "<style\n  type=\"text/css\">\n\nfoo\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn block_followed_by_paragraph() {
    let input = "<script>code</script>\nokay\n";
    let tokens = parse(input);
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert_eq!(block, "<script>code</script>\n");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("okay"), "got {:?}", text);
}

#[test]
fn block_with_one_space_indent() {
    let input = " <script>foo</script>\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with(" <script>"), "got {:?}", block);
}

#[test]
fn block_with_three_space_indent() {
    let input = "   <pre>foo</pre>\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with("   <pre>"), "got {:?}", block);
}

#[test]
fn four_space_indent_is_code_block_not_raw_html() {
    let tokens = parse("    <script>foo</script>\n");
    assert!(first_html_block(&tokens).is_none());
    assert!(tokens.iter().any(|t| matches!(t, Token::Code { block: true, .. })));
}

#[test]
fn opener_mid_paragraph_stays_inline() {
    // `<script>` not at line start should not become a block.
    let tokens = parse("paragraph <script>code</script>\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn opener_followed_by_space_is_valid() {
    // Opener `<pre ` (with space before `>`) is a valid block opener
    // per spec.
    let input = "<pre >\nbody\n</pre>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn opener_followed_by_newline_is_valid() {
    // Opener with no attributes and a newline immediately after the
    // tag name (no `>` on the opener line).
    let input = "<pre\n>\nbody\n</pre>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn block_inside_blockquote() {
    let tokens = parse("> <script>code</script>\n");
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
fn tag_name_followed_by_letter_is_not_opener() {
    // `<scripter>` is NOT a raw-content opener because `r` follows the
    // tag name — the delimiter requirement (space/tab/`>`/newline)
    // isn't met.
    let tokens = parse("<scripter>code</scripter>\n");
    assert!(first_html_block(&tokens).is_none());
}
