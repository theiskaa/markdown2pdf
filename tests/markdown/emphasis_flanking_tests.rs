use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn emphasis_with_inner_spaces_does_not_open() {
    // `* foo *` — the opening `*` is followed by a space, so it can't
    // open emphasis (not left-flanking). Should be plain text.
    let tokens = parse("a * foo * b");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    let body = Token::collect_all_text(&tokens);
    assert!(body.contains("* foo *"), "got {:?}", body);
}

#[test]
fn opener_followed_by_space_no_emphasis() {
    let tokens = parse("a* foo*");
    // Opener is `*` followed by space → not left-flanking → no emphasis.
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn closer_preceded_by_space_no_emphasis() {
    let tokens = parse("a *foo *");
    // Closing `*` is preceded by space → not right-flanking → no close.
    assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn valid_emphasis_with_no_inner_space() {
    let tokens = parse("a *foo* b");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Emphasis { level: 1, .. }))
    );
}

#[test]
fn valid_strong_with_no_inner_space() {
    let tokens = parse("a **bold** b");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Emphasis { level: 2, .. }))
    );
}

#[test]
fn underscore_emphasis_works_at_word_boundary() {
    let tokens = parse("a _foo_ b");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Emphasis { level: 1, .. }))
    );
}

#[test]
fn intra_word_underscore_still_text() {
    // `_` flanked by alphanumerics on both sides is treated as literal text.
    let tokens = parse("foo_bar_baz");
    assert_eq!(tokens, vec![Token::Text("foo_bar_baz".to_string())]);
}

#[test]
fn star_can_open_intra_word() {
    // `*` is more permissive than `_` per spec: it can open intra-word.
    let tokens = parse("foo*bar*baz");
    assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn unmatched_lone_asterisk_still_text() {
    // A stray asterisk must not abort — it falls back to literal text.
    let tokens = parse("Use * for bullets.");
    let body = Token::collect_all_text(&tokens);
    assert_eq!(body, "Use * for bullets.");
}

#[test]
fn emphasis_does_not_cross_blank_line() {
    // An opener that can't find a valid same-paragraph closer must NOT
    // gobble the next paragraph's content. The blank line acts as a
    // paragraph boundary, forcing literal-text fallback.
    let input = "para with *unclosed opener\n\n## Heading after blank";
    let tokens = parse(input);
    // The `## Heading…` must parse as a real heading token.
    let has_heading = tokens.iter().any(|t| matches!(t, Token::Heading(_, 2)));
    assert!(
        has_heading,
        "expected H2 after blank line, got {:?}",
        tokens
    );
    // Body must still contain the `*` literally.
    let body = Token::collect_all_text(&tokens);
    assert!(body.contains("*unclosed opener"), "got {:?}", body);
}

#[test]
fn star_with_inner_space_does_not_eat_following_paragraph() {
    // `*foo *` cannot close (closer preceded by space) and must not
    // gobble the next heading.
    let input = "Closer preceded: a *foo * — text.\n\n## Next heading";
    let tokens = parse(input);
    let has_heading = tokens.iter().any(|t| matches!(t, Token::Heading(_, 2)));
    assert!(
        has_heading,
        "expected H2 after the paragraph, got {:?}",
        tokens
    );
}
