//! List-item corner cases. Targets `parse_list_item`,
//! `check_ordered_list_marker`, `line_starts_with_list_marker`.

use markdown2pdf::markdown::*;

use super::common::parse;


fn count_items(input: &str) -> usize {
    parse(input)
        .iter()
        .filter(|t| matches!(t, Token::ListItem { .. }))
        .count()
}

#[test]
fn bullet_markers_recognized() {
    for marker in ["-", "+", "*"] {
        let input = format!("{} a\n", marker);
        assert_eq!(count_items(&input), 1, "marker {:?}", marker);
    }
}

#[test]
fn ordered_marker_with_dot() {
    let tokens = parse("1. a\n");
    let item = tokens.iter().find(|t| matches!(t, Token::ListItem { .. })).unwrap();
    if let Token::ListItem { number, ordered, marker, .. } = item {
        assert_eq!(*number, Some(1));
        assert!(ordered);
        assert_eq!(*marker, '.');
    }
}

#[test]
fn ordered_marker_with_paren() {
    let tokens = parse("1) a\n");
    let item = tokens.iter().find(|t| matches!(t, Token::ListItem { .. })).unwrap();
    if let Token::ListItem { marker, .. } = item {
        assert_eq!(*marker, ')');
    }
}

#[test]
fn ordered_list_starting_at_zero() {
    let tokens = parse("0. a\n");
    let item = tokens.iter().find(|t| matches!(t, Token::ListItem { .. })).unwrap();
    if let Token::ListItem { number, .. } = item {
        assert_eq!(*number, Some(0));
    }
}

#[test]
fn marker_change_splits_lists() {
    assert_eq!(count_items("- a\n+ b\n"), 2);
}

#[test]
fn ordered_then_bullet_splits_lists() {
    assert_eq!(count_items("1. a\n- b\n"), 2);
}

#[test]
fn empty_list_item_does_not_panic() {
    let _ = parse("-\n");
}

#[test]
fn nine_digit_ordered_marker() {
    assert_eq!(count_items("999999999. a\n"), 1);
}

#[test]
fn ten_digit_ordered_marker_is_paragraph() {
    assert_eq!(count_items("1234567890. a\n"), 0);
}

#[test]
fn item_with_marker_then_blockquote() {
    let tokens = parse("- > inner quote\n");
    let item = tokens.iter().find(|t| matches!(t, Token::ListItem { .. })).unwrap();
    if let Token::ListItem { content, .. } = item {
        assert!(content.iter().any(|t| matches!(t, Token::BlockQuote(_))));
    }
}

#[test]
fn item_with_indented_continuation() {
    let tokens = parse("- first\n  continued\n");
    let item = tokens.iter().find(|t| matches!(t, Token::ListItem { .. })).unwrap();
    if let Token::ListItem { content, .. } = item {
        let text = Token::collect_all_text(content);
        assert!(text.contains("first"));
        assert!(text.contains("continued"));
    }
}

#[test]
fn task_list_item_basic() {
    let tokens = parse("- [ ] task\n");
    let item = tokens.iter().find(|t| matches!(t, Token::ListItem { .. })).unwrap();
    if let Token::ListItem { checked, .. } = item {
        assert_eq!(*checked, Some(false));
    }
}
