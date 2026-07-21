//! Thematic-break corner cases. Targets `is_thematic_break_line` and
//! `check_horizontal_rule`. These complement the spec-runner roll-up
//! with focused, line-anchored assertions.

use markdown2pdf::markdown::*;

use super::common::parse;

fn has_hr(tokens: &[Token]) -> bool {
    tokens.iter().any(|t| matches!(t, Token::HorizontalRule))
}

#[test]
fn three_dashes() {
    assert!(has_hr(&parse("---\n")));
}

#[test]
fn three_stars() {
    assert!(has_hr(&parse("***\n")));
}

#[test]
fn three_underscores() {
    assert!(has_hr(&parse("___\n")));
}

#[test]
fn more_than_three_markers() {
    assert!(has_hr(&parse("------\n")));
    assert!(has_hr(&parse("******\n")));
    assert!(has_hr(&parse("______\n")));
}

#[test]
fn spaces_between_markers_allowed() {
    assert!(has_hr(&parse("- - -\n")));
    assert!(has_hr(&parse("* * *\n")));
}

#[test]
fn cannot_mix_markers() {
    // Mixed markers don't form a thematic break.
    assert!(!has_hr(&parse("-*-\n")));
    assert!(!has_hr(&parse("- * _\n")));
}

#[test]
fn intervening_non_whitespace_disallows() {
    assert!(!has_hr(&parse("-x-\n")));
    assert!(!has_hr(&parse("--a\n")));
}

#[test]
fn four_space_indent_blocks_thematic_break() {
    // 4-space indent → indented code block, not HR.
    assert!(!has_hr(&parse("    ---\n")));
}

#[test]
fn one_to_three_space_indent_allowed() {
    assert!(has_hr(&parse(" ---\n")));
    assert!(has_hr(&parse("  ---\n")));
    assert!(has_hr(&parse("   ---\n")));
}

#[test]
fn hr_inside_blockquote() {
    let tokens = parse("> ---\n");
    let Some(Token::BlockQuote(body)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    assert!(has_hr(body));
}
