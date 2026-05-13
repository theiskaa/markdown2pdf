use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn double_backtick_inline_with_single_backtick_inside() {
    let tokens = parse("``code with ` inside``");
    assert_eq!(
        tokens,
        vec![Token::Code { language: "".to_string(), content: "code with ` inside".to_string(), block: false }]
    );
}

#[test]
fn triple_backtick_inline_when_not_at_line_start() {
    let tokens = parse("inline ```code with `` inside``` here");
    // First Text("inline "), then Code, then Text(" here").
    assert!(matches!(tokens[0], Token::Text(ref s) if s.contains("inline")));
    assert!(matches!(tokens[1], Token::Code { ref content, .. } if content.contains("``")));
}

#[test]
fn double_backtick_with_count_mismatch_inside() {
    // ``a`b``  -> code containing "a`b". A single ` inside doesn't close.
    let tokens = parse("``a`b``");
    assert_eq!(
        tokens,
        vec![Token::Code { language: "".to_string(), content: "a`b".to_string(), block: false }]
    );
}

#[test]
fn fenced_block_still_works() {
    let input = "```rust\nfn main() {}\n```";
    let tokens = parse(input);
    assert_eq!(
        tokens,
        vec![Token::Code { language: "rust".to_string(), content: "fn main() {}".to_string(), block: true }]
    );
}

#[test]
fn fenced_block_preserves_inner_backticks() {
    // A single ` (or any run shorter than the opener) inside the body
    // must remain in the output. Pre-existing bug: count_backticks
    // advanced past the inner ticks but never pushed them to content.
    let input = "```rust\nlet s = `template`;\n```";
    let tokens = parse(input);
    if let Token::Code { content: body, .. } = &tokens[0] {
        assert!(
            body.contains("`template`"),
            "fenced block stripped inner backticks: {:?}",
            body
        );
    } else {
        panic!("expected Code, got {:?}", tokens);
    }
}

#[test]
fn fenced_block_preserves_double_backtick_run_inside() {
    // Triple-fence; body contains `` (count 2) which must survive.
    let input = "```\nfoo `` bar\n```";
    let tokens = parse(input);
    if let Token::Code { content: body, .. } = &tokens[0] {
        assert!(
            body.contains("``"),
            "double-backtick run lost in fence body: {:?}",
            body
        );
    } else {
        panic!("expected Code, got {:?}", tokens);
    }
}

#[test]
fn double_backtick_at_line_start_with_content_is_inline() {
    // ``code`` at line start is still inline if there's content on the
    // same line beyond the closing run.
    let tokens = parse("``inline`` plus text");
    assert!(matches!(tokens[0], Token::Code { ref content, .. } if content == "inline"));
    assert!(tokens.iter().any(|t| matches!(t, Token::Text(s) if s.contains("plus text"))));
}

#[test]
fn unclosed_inline_code_falls_back_to_text() {
    // No matching closer (EOF reached) — the opener run
    // becomes literal text so the body chars still render normally.
    let tokens = parse("``never closes");
    assert!(matches!(tokens[0], Token::Text(ref s) if s == "``"));
    let body = Token::collect_all_text(&tokens);
    assert!(body.contains("never closes"), "got {:?}", body);
}

#[test]
fn unclosed_inline_code_does_not_gobble_across_blank_line() {
    // An unclosed `` ` `` inside a paragraph must NOT pull the next
    // paragraph's text into a code block. The literal-text fallback
    // prevents the gobble.
    let input = "first paragraph with `unclosed.\n\nSecond paragraph.";
    let tokens = parse(input);
    // No multi-line Code should be produced.
    let multi_line_code = tokens
        .iter()
        .any(|t| matches!(t, Token::Code { content: c, .. } if c.contains('\n')));
    assert!(
        !multi_line_code,
        "code span gobbled across paragraphs: {:?}",
        tokens
    );
    let body = Token::collect_all_text(&tokens);
    assert!(body.contains("Second paragraph"), "got {:?}", body);
}

#[test]
fn single_backtick_unchanged() {
    let tokens = parse("`simple`");
    assert_eq!(
        tokens,
        vec![Token::Code { language: "".to_string(), content: "simple".to_string(), block: false }]
    );
}
