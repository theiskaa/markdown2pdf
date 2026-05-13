use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn bullet_item_with_two_paragraphs() {
    let tokens = parse("- foo\n\n  bar");
    assert_eq!(
        tokens.iter().filter(|t| matches!(t, Token::ListItem { .. })).count(),
        1,
        "expected exactly one list item, got {}",
        Token::slice_to_compact(&tokens)
    );
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.contains("foo"), "got {:?}", text);
        assert!(
            text.contains("bar"),
            "second paragraph must be inside the item: {:?}",
            text
        );
    } else {
        panic!("expected ListItem, got {}", Token::slice_to_compact(&tokens));
    }
}

#[test]
fn under_indented_continuation_starts_top_level_paragraph() {
    // `bar` is at column 0 (no indent) — must NOT be inside the item.
    let tokens = parse("- foo\n\nbar");
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.contains("foo"));
        assert!(!text.contains("bar"), "bar leaked into item: {:?}", text);
    }
    // bar must appear as a separate top-level token.
    let after = Token::collect_all_text(&tokens[1..]);
    assert!(after.contains("bar"), "bar missing from rest");
}

#[test]
fn bullet_item_with_blank_makes_list_loose() {
    // The blank-line-between-paragraphs inside an item makes the list
    // loose per spec ("any item directly contains two block-level
    // elements with a blank line between them").
    let tokens = parse("- foo\n\n  bar\n- second");
    let loose_flags: Vec<bool> = tokens
        .iter()
        .filter_map(|t| {
            if let Token::ListItem { loose, .. } = t {
                Some(*loose)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(loose_flags, vec![true, true], "got {}", Token::slice_to_compact(&tokens));
}

#[test]
fn ordered_item_with_two_paragraphs() {
    // For ordered `1. ` the content offset is col 3.
    let tokens = parse("1. first\n\n   second");
    assert_eq!(
        tokens.iter().filter(|t| matches!(t, Token::ListItem { .. })).count(),
        1,
        "got {}",
        Token::slice_to_compact(&tokens)
    );
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.contains("first") && text.contains("second"), "got {:?}", text);
    }
}

#[test]
fn item_with_only_one_paragraph_unchanged() {
    // Regression: single-paragraph items must not change shape.
    let tokens = parse("- only");
    assert_eq!(
        tokens.iter().filter(|t| matches!(t, Token::ListItem { .. })).count(),
        1
    );
    if let Token::ListItem { content, loose, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert_eq!(text, "only");
        assert!(!loose);
    }
}

#[test]
fn three_paragraphs_in_one_item() {
    let tokens = parse("- a\n\n  b\n\n  c");
    assert_eq!(
        tokens.iter().filter(|t| matches!(t, Token::ListItem { .. })).count(),
        1,
        "got {}",
        Token::slice_to_compact(&tokens)
    );
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        for needle in &["a", "b", "c"] {
            assert!(text.contains(needle), "{:?} missing from {:?}", needle, text);
        }
    }
}
