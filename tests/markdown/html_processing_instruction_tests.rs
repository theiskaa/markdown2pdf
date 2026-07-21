//! HTML processing-instruction blocks (CommonMark §4.6 type 3).
//!
//! Opener: `<?` at line start (0–3 space indent allowed).
//! Body: runs to `?>` on this or a subsequent line.
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
fn php_pi_single_line() {
    let input = "<?php echo 'hi'; ?>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn pi_multi_line_with_blank_inside() {
    // Spec example 180 shape.
    let input = "<?php\n\n  echo '>';\n\n?>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn pi_followed_by_paragraph() {
    let input = "<?php\necho 'x';\n?>\nokay\n";
    let tokens = parse(input);
    let block = first_html_block(&tokens).expect("expected HtmlBlock");
    assert_eq!(block, "<?php\necho 'x';\n?>\n");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("okay"), "got {:?}", text);
}

#[test]
fn xml_processing_instruction() {
    let input = "<?xml version=\"1.0\"?>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn pi_with_question_marks_inside() {
    // A bare `?` (not followed by `>`) inside the body must NOT
    // close the block — the full 2-char `?>` terminator is required.
    let input = "<?php is ? a single char until?>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
}

#[test]
fn pi_with_markdown_chars_left_literal() {
    let input = "<?php\n*not emphasis*\n`not code`\n?>\n";
    let tokens = parse(input);
    assert_eq!(first_html_block(&tokens).as_deref(), Some(input));
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn pi_with_one_space_indent() {
    let input = " <?php ?>\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with(" <?"), "got {:?}", block);
}

#[test]
fn pi_with_three_space_indent() {
    let input = "   <?php ?>\n";
    let block = first_html_block(&parse(input)).expect("expected HtmlBlock");
    assert!(block.starts_with("   <?"), "got {:?}", block);
}

#[test]
fn four_space_indent_is_code_block_not_pi() {
    let tokens = parse("    <?php ?>\n");
    assert!(first_html_block(&tokens).is_none());
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Code { block: true, .. }))
    );
}

#[test]
fn pi_not_at_line_start_is_inline_text() {
    let tokens = parse("paragraph <?php ?>\n");
    assert!(first_html_block(&tokens).is_none());
}

#[test]
fn pi_inside_blockquote() {
    let tokens = parse("> <?php ?>\n");
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
fn unterminated_pi_falls_through() {
    let tokens = parse("<?php never closes\nmore text\n");
    assert!(first_html_block(&tokens).is_none());
}
