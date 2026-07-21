use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn indented_continuation_belongs_to_item() {
    let input = "- item one\n  continues here\n- item two";
    let tokens = parse(input);
    let li_count = tokens
        .iter()
        .filter(|t| matches!(t, Token::ListItem { .. }))
        .count();
    assert_eq!(li_count, 2, "got {:?}", tokens);
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.contains("item one"), "got {:?}", text);
        assert!(text.contains("continues here"), "got {:?}", text);
    }
}

#[test]
fn zero_indent_lazy_continuation() {
    // a non-blank, non-marker line at indent 0 still
    // continues the previous item's paragraph.
    let input = "- item one\nlazy line\n- item two";
    let tokens = parse(input);
    let li_count = tokens
        .iter()
        .filter(|t| matches!(t, Token::ListItem { .. }))
        .count();
    assert_eq!(li_count, 2, "got {:?}", tokens);
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.contains("lazy line"), "got {:?}", text);
    }
}

#[test]
fn blank_line_ends_list_item() {
    let input = "- item one\n\n- item two";
    let tokens = parse(input);
    let li_count = tokens
        .iter()
        .filter(|t| matches!(t, Token::ListItem { .. }))
        .count();
    // Two items either way; ensure first item didn't gobble blank.
    assert_eq!(li_count, 2);
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(
            !text.contains("item two"),
            "first item should not include second"
        );
    }
}

#[test]
fn heading_line_terminates_item() {
    let input = "- item one\n# heading";
    let tokens = parse(input);
    assert!(tokens.iter().any(|t| matches!(t, Token::Heading(_, 1))));
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(
            !text.contains("heading"),
            "heading shouldn't be inside item"
        );
    }
}

#[test]
fn thematic_break_terminates_item() {
    let input = "- item one\n---";
    let tokens = parse(input);
    assert!(
        tokens.iter().any(|t| matches!(t, Token::HorizontalRule)),
        "expected HR, got {:?}",
        tokens
    );
}

#[test]
fn nested_list_still_works() {
    let input = "- Item 1\n  - Nested 1\n  - Nested 2\n- Item 2";
    let tokens = parse(input);
    let top_li = tokens
        .iter()
        .filter(|t| matches!(t, Token::ListItem { .. }))
        .count();
    assert_eq!(top_li, 2, "got {:?}", tokens);
}

#[test]
fn simple_two_items_unchanged() {
    let input = "- a\n- b";
    let tokens = parse(input);
    assert_eq!(
        tokens
            .iter()
            .filter(|t| matches!(t, Token::ListItem { .. }))
            .count(),
        2
    );
}
