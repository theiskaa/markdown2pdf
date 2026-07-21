use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn lone_asterisk_in_paragraph_is_text() {
    let tokens = parse("Use * for bullets.");
    let text = Token::collect_all_text(&tokens);
    assert_eq!(text, "Use * for bullets.");
}

#[test]
fn lone_underscore_in_paragraph_is_text() {
    // Note: trailing _ after a space is left-flanking and tries to open;
    // with no closer, it must fall back to literal text.
    let tokens = parse("Lone _underscore here");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("_underscore here"), "got {:?}", text);
}

#[test]
fn unmatched_double_asterisk() {
    let tokens = parse("This **bold start has no end");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("**bold start"), "got {:?}", text);
}

#[test]
fn stray_asterisk_at_eof() {
    let tokens = parse("trailing *");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("*"), "got {:?}", text);
}

#[test]
fn stray_underscore_at_eof() {
    let tokens = parse("trailing _");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("_"), "got {:?}", text);
}

#[test]
fn stray_then_valid_emphasis() {
    // The first * is unmatched -> literal; the *real* pair is emphasis.
    let tokens = parse("stray * then *real* pair");
    // Must contain at least one Emphasis somewhere
    assert!(
        tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })),
        "expected emphasis somewhere in {:?}",
        tokens
    );
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("real"), "got {:?}", text);
}

#[test]
fn valid_then_stray_emphasis() {
    let tokens = parse("*good* then a stray *");
    // Token 0 should be a real emphasis, last token is plain text containing *.
    assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("*"), "got {:?}", text);
}

#[test]
fn stray_in_heading() {
    let tokens = parse("# heading with * stray");
    assert!(matches!(tokens[0], Token::Heading(_, 1)));
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("*"), "got {:?}", text);
}

#[test]
fn stray_in_list_item() {
    let tokens = parse("- item with * stray");
    assert!(matches!(tokens[0], Token::ListItem { .. }));
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("*"), "got {:?}", text);
}

#[test]
fn triple_asterisk_no_close() {
    let tokens = parse("***boldital with no closer");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("***"), "got {:?}", text);
    assert!(text.contains("boldital"), "got {:?}", text);
}

#[test]
fn regression_basic_italic() {
    let tokens = parse("*italic*");
    assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
}

#[test]
fn regression_basic_bold() {
    let tokens = parse("**bold**");
    assert!(matches!(tokens[0], Token::Emphasis { level: 2, .. }));
}

#[test]
fn regression_underscore_emphasis() {
    let tokens = parse("_italic_ and __bold__");
    let count = tokens
        .iter()
        .filter(|t| matches!(t, Token::Emphasis { .. }))
        .count();
    assert_eq!(count, 2, "expected two emphasis tokens, got {:?}", tokens);
}

#[test]
fn regression_intra_word_underscore_still_text() {
    let tokens = parse("phpmyadmin/localized_docs");
    assert_eq!(
        tokens,
        vec![Token::Text("phpmyadmin/localized_docs".to_string())]
    );
}

#[test]
fn document_with_stray_does_not_lose_other_tokens() {
    let input = "# Title\n\nBody has * stray and `code` and [link](url).";
    let tokens = parse(input);
    assert!(matches!(tokens[0], Token::Heading(_, 1)));
    // Code span and link must still parse despite the stray *.
    assert!(tokens.iter().any(|t| matches!(t, Token::Code { .. })));
    assert!(tokens.iter().any(|t| matches!(t, Token::Link { .. })));
}
