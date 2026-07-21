use markdown2pdf::markdown::*;

use super::common::parse;

fn items(tokens: &[Token]) -> Vec<bool> {
    tokens
        .iter()
        .filter_map(|t| {
            if let Token::ListItem { loose, .. } = t {
                Some(*loose)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn tight_bullet_list_marks_no_items_loose() {
    let tokens = parse("- a\n- b\n- c");
    assert_eq!(items(&tokens), vec![false, false, false]);
}

#[test]
fn blank_line_between_items_marks_list_loose() {
    let tokens = parse("- a\n\n- b\n\n- c");
    assert_eq!(items(&tokens), vec![true, true, true]);
}

#[test]
fn single_blank_anywhere_makes_whole_list_loose() {
    // Spec: even one blank-separated pair makes ALL items loose.
    let tokens = parse("- a\n- b\n\n- c");
    assert_eq!(items(&tokens), vec![true, true, true]);
}

#[test]
fn tight_ordered_list() {
    let tokens = parse("1. one\n2. two\n3. three");
    assert_eq!(items(&tokens), vec![false, false, false]);
}

#[test]
fn loose_ordered_list() {
    let tokens = parse("1. one\n\n2. two");
    assert_eq!(items(&tokens), vec![true, true]);
}

#[test]
fn single_item_list_is_tight() {
    let tokens = parse("- solo");
    assert_eq!(items(&tokens), vec![false]);
}

#[test]
fn tight_nested_list_keeps_inner_tight() {
    let input = "- outer1\n  - in1\n  - in2\n- outer2";
    let tokens = parse(input);
    assert_eq!(items(&tokens), vec![false, false]);
    if let Token::ListItem { content, .. } = &tokens[0] {
        assert_eq!(items(content), vec![false, false], "inner: {:?}", content);
    } else {
        panic!("expected ListItem");
    }
}

#[test]
fn nested_list_blank_makes_both_levels_loose() {
    // Spec: a list is loose if any item directly contains two block-level
    // elements with a blank line between them. The two inner ListItems
    // inside outer1 are separated by a blank, so the inner list AND the
    // outer list are both loose.
    let input = "- outer1\n  - inner1\n\n  - inner2\n- outer2";
    let tokens = parse(input);
    assert_eq!(
        items(&tokens),
        vec![true, true],
        "outer: {}",
        Token::slice_to_compact(&tokens)
    );
    if let Token::ListItem { content, .. } = &tokens[0] {
        let inner = items(content);
        assert_eq!(
            inner,
            vec![true, true],
            "inner items: {}",
            Token::slice_to_compact(content)
        );
    } else {
        panic!(
            "expected ListItem, got {}",
            Token::slice_to_compact(&tokens)
        );
    }
}

#[test]
fn outer_loose_inner_tight() {
    let input = "- outer1\n  - in1\n  - in2\n\n- outer2";
    let tokens = parse(input);
    assert_eq!(items(&tokens), vec![true, true]);
    if let Token::ListItem { content, .. } = &tokens[0] {
        let inner = items(content);
        assert_eq!(inner, vec![false, false], "inner items: {:?}", content);
    } else {
        panic!("expected ListItem");
    }
}

#[test]
fn list_in_blockquote_loose_detected() {
    let input = "> - a\n>\n> - b";
    let tokens = parse(input);
    if let Token::BlockQuote(body) = &tokens[0] {
        assert_eq!(
            items(body),
            vec![true, true],
            "quote body: {}",
            Token::slice_to_compact(body)
        );
    } else {
        panic!(
            "expected BlockQuote, got {}",
            Token::slice_to_compact(&tokens)
        );
    }
}

#[test]
fn two_separate_lists_each_have_own_loose_flag() {
    // A blank line followed by content that isn't another item ends the
    // list. The next list starts fresh.
    let input = "- a\n- b\n\nparagraph\n\n- c\n\n- d";
    let tokens = parse(input);
    let item_states: Vec<bool> = tokens
        .iter()
        .filter_map(|t| {
            if let Token::ListItem { loose, .. } = t {
                Some(*loose)
            } else {
                None
            }
        })
        .collect();
    // First list (a, b): tight. Second list (c, d): loose.
    assert_eq!(item_states, vec![false, false, true, true]);
}

#[test]
fn task_list_loose_detection() {
    let input = "- [ ] task1\n\n- [x] task2";
    let tokens = parse(input);
    assert_eq!(items(&tokens), vec![true, true]);
}
