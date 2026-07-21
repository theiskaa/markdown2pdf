use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn two_trailing_spaces_produce_hard_break() {
    let tokens = parse("first  \nsecond");
    assert!(
        tokens.iter().any(|t| matches!(t, Token::HardBreak)),
        "expected HardBreak, got {:?}",
        tokens
    );
    // Trailing spaces should be stripped from the preceding Text.
    if let Token::Text(s) = &tokens[0] {
        assert!(!s.ends_with(' '), "trailing spaces not stripped: {:?}", s);
    }
}

#[test]
fn three_trailing_spaces_also_hard_break() {
    let tokens = parse("first   \nsecond");
    assert!(tokens.iter().any(|t| matches!(t, Token::HardBreak)));
}

#[test]
fn one_trailing_space_is_soft_break() {
    let tokens = parse("first \nsecond");
    assert!(!tokens.iter().any(|t| matches!(t, Token::HardBreak)));
    assert!(tokens.iter().any(|t| matches!(t, Token::Newline)));
}

#[test]
fn no_trailing_space_is_soft_break() {
    let tokens = parse("first\nsecond");
    assert!(!tokens.iter().any(|t| matches!(t, Token::HardBreak)));
    assert!(tokens.iter().any(|t| matches!(t, Token::Newline)));
}

#[test]
fn trailing_backslash_is_hard_break() {
    let tokens = parse("first\\\nsecond");
    assert!(
        tokens.iter().any(|t| matches!(t, Token::HardBreak)),
        "expected HardBreak from trailing \\, got {:?}",
        tokens
    );
    // The backslash itself must be stripped from the preceding Text.
    if let Token::Text(s) = &tokens[0] {
        assert!(!s.ends_with('\\'), "backslash not stripped: {:?}", s);
    }
}

#[test]
fn escaped_backslash_then_newline_is_soft_break() {
    // `\\\n` is an escaped backslash (literal `\`) plus a soft break,
    // NOT a hard break (the trailing char isn't a "lone" backslash).
    let tokens = parse("first\\\\\nsecond");
    assert!(!tokens.iter().any(|t| matches!(t, Token::HardBreak)));
    // The literal backslash must remain in the Text.
    if let Token::Text(s) = &tokens[0] {
        assert!(s.contains('\\'), "literal backslash dropped: {:?}", s);
    }
}

#[test]
fn hard_break_inside_blockquote() {
    let tokens = parse("> line one  \n> line two");
    if let Token::BlockQuote(body) = &tokens[0] {
        assert!(body.iter().any(|t| matches!(t, Token::HardBreak)));
    } else {
        panic!("expected BlockQuote, got {:?}", tokens);
    }
}

#[test]
fn hard_break_in_list_item() {
    let tokens = parse("- item one  \n  continuation");
    // Just ensure no error and the HardBreak appears somewhere.
    let any_hb = tokens.iter().any(|t| matches!(t, Token::HardBreak))
        || matches!(&tokens[0], Token::ListItem { content, .. }
            if content.iter().any(|t| matches!(t, Token::HardBreak)));
    assert!(any_hb, "expected HardBreak somewhere, got {:?}", tokens);
}

#[test]
fn no_hard_break_in_atx_heading() {
    // Headings are single-line; trailing spaces are not a hard break.
    let tokens = parse("# Heading  \nbody");
    // Heading content shouldn't contain HardBreak.
    if let Token::Heading(content, _) = &tokens[0] {
        assert!(!content.iter().any(|t| matches!(t, Token::HardBreak)));
    }
}
