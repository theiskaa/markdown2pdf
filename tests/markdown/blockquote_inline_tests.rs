use markdown2pdf::markdown::*;

use super::common::parse;

fn block_body(t: &Token) -> &Vec<Token> {
    if let Token::BlockQuote(body) = t {
        body
    } else {
        panic!("expected BlockQuote, got {:?}", t);
    }
}

#[test]
fn inline_emphasis_inside_quote() {
    let tokens = parse("> use **bold** here");
    assert_eq!(tokens.len(), 1);
    let body = block_body(&tokens[0]);
    // Body must contain a real emphasis token, not raw "**bold**" text.
    assert!(
        body.iter()
            .any(|t| matches!(t, Token::Emphasis { level: 2, .. })),
        "expected emphasis inside quote, got body {:?}",
        body
    );
}

#[test]
fn inline_code_inside_quote() {
    let tokens = parse("> see `the_code` for details");
    let body = block_body(&tokens[0]);
    assert!(
        body.iter().any(|t| matches!(t, Token::Code { .. })),
        "expected code span, got body {:?}",
        body
    );
}

#[test]
fn inline_link_inside_quote() {
    let tokens = parse("> visit [example](https://example.com)");
    let body = block_body(&tokens[0]);
    assert!(
        body.iter().any(|t| matches!(t, Token::Link { .. })),
        "expected link inside quote, got body {:?}",
        body
    );
}

#[test]
fn intra_word_underscore_inside_quote() {
    let tokens = parse("> Quote with foo_bar inside");
    let body = block_body(&tokens[0]);
    let text = Token::collect_all_text(body);
    assert!(text.contains("foo_bar"), "got {:?}", text);
    // Should NOT have produced an emphasis token.
    assert!(!body.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn two_line_quote_merges_into_one() {
    let tokens = parse("> first\n> second");
    // One BlockQuote with both lines as content (text/newline structure
    // is fine, but we should NOT have two BlockQuote tokens).
    let count = tokens
        .iter()
        .filter(|t| matches!(t, Token::BlockQuote(_)))
        .count();
    assert_eq!(count, 1, "expected one merged blockquote, got {:?}", tokens);
    let body = block_body(&tokens[0]);
    let text = Token::collect_all_text(body);
    assert!(text.contains("first"), "got {:?}", text);
    assert!(text.contains("second"), "got {:?}", text);
}

#[test]
fn multi_line_with_emphasis_spanning_lines() {
    let tokens = parse("> _start\n> end_");
    let body = block_body(&tokens[0]);
    // Emphasis wraps "start\nend" (across the line break)
    assert!(
        body.iter().any(|t| matches!(t, Token::Emphasis { .. })),
        "expected emphasis spanning lines, got {:?}",
        body
    );
}

#[test]
fn blank_line_breaks_blockquote() {
    let tokens = parse("> first\n\n> second");
    let count = tokens
        .iter()
        .filter(|t| matches!(t, Token::BlockQuote(_)))
        .count();
    assert_eq!(
        count, 2,
        "blank line should separate quotes, got {:?}",
        tokens
    );
}

#[test]
fn empty_quote_marker() {
    // A bare `>` followed by EOL is valid CommonMark — empty quote.
    let tokens = parse(">");
    assert!(matches!(tokens[0], Token::BlockQuote(_)));
}

#[test]
fn quote_with_no_space_after_marker() {
    // `>foo` is also a blockquote (the space is optional).
    let tokens = parse(">foo");
    assert!(matches!(tokens[0], Token::BlockQuote(_)));
    let body = block_body(&tokens[0]);
    let text = Token::collect_all_text(body);
    assert!(text.contains("foo"), "got {:?}", text);
}

#[test]
fn regression_simple_quote_text_still_present() {
    let tokens = parse("> This is a quote");
    let body = block_body(&tokens[0]);
    let text = Token::collect_all_text(body);
    assert!(text.contains("This is a quote"), "got {:?}", text);
}

#[test]
fn paragraph_then_quote_then_paragraph() {
    let input = "first\n> middle\nlast";
    let tokens = parse(input);
    let bq_count = tokens
        .iter()
        .filter(|t| matches!(t, Token::BlockQuote(_)))
        .count();
    assert_eq!(bq_count, 1, "expected exactly one quote, got {:?}", tokens);
}
