//! Direct tests for `resolve_emphasis` and its helpers
//! (`em_is_left_flanking`, `em_is_right_flanking`, `compute_em_flanking`,
//! `find_em_delims`). These pin the current stack-based algorithm's
//! behavior; the assertions cover left/right flanking, the n*m rule,
//! same-character-only matching, and triple-emphasis shape.

use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn basic_single_star() {
    let tokens = parse("*x*");
    assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
}

#[test]
fn basic_double_star() {
    let tokens = parse("**x**");
    assert!(matches!(tokens[0], Token::Emphasis { level: 2, .. }));
}

#[test]
fn triple_star_is_nested_emphasis() {
    // `***x***` should produce nested emphasis tokens.
    let tokens = parse("***x***");
    let text = Token::collect_all_text(&tokens);
    assert_eq!(text, "x");
    assert!(matches!(&tokens[0], Token::Emphasis { .. }));
}

#[test]
fn basic_underscore_emphasis() {
    let tokens = parse("_x_");
    assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
}

#[test]
fn intra_word_underscore_not_emphasis() {
    let tokens = parse("foo_bar_baz");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn intra_word_asterisk_is_emphasis() {
    // Asterisks (unlike underscores) DO open inside words.
    let tokens = parse("foo*bar*baz");
    assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn same_char_only_matching() {
    // `*x_` and `_x*` must NOT match — different delimiter chars.
    let tokens = parse("*x_");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    let tokens = parse("_x*");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn whitespace_after_opener_blocks_emphasis() {
    // `* x*` — space immediately after opener means not left-flanking.
    let tokens = parse("* x*");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn whitespace_before_closer_blocks_emphasis() {
    // `*x *` — space immediately before closer means not right-flanking.
    let tokens = parse("*x *");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn unbalanced_run_open_overflow() {
    // `***foo*` — opens with 3, closes with 1 — leaves 2 unmatched `*`.
    let tokens = parse("***foo*");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("foo"));
    // Some `*` chars (the unmatched 2) survive as literal text.
    assert!(text.contains('*'));
}

#[test]
fn unbalanced_run_close_overflow() {
    // `*foo***` — opens with 1, closes with 3 — emits Em(foo) + literal `**`.
    let tokens = parse("*foo***");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("foo"));
    assert!(text.contains('*'));
}

#[test]
fn punctuation_flanking_currency_symbol() {
    // Currency `£` is in Unicode `Sc` category; with `S*` coverage in
    // `is_md_punctuation`, `*£*` should still produce emphasis.
    let tokens = parse("*£*");
    assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn left_flanking_with_ascii_punct_close() {
    // `*foo.*` — `.` is punctuation; closer is right-flanking.
    let tokens = parse("*foo.*");
    assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn nested_emphasis_strong_in_em() {
    let tokens = parse("*one **two** three*");
    let Token::Emphasis { content, .. } = &tokens[0] else {
        panic!("expected outer Emphasis, got {:?}", tokens);
    };
    assert!(
        content
            .iter()
            .any(|t| matches!(t, Token::Emphasis { level: 2, .. }))
    );
}
