use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn paren_marker_creates_ordered_list_item() {
    let tokens = parse("1) one\n2) two");
    let count = tokens
        .iter()
        .filter(|t| matches!(t, Token::ListItem { ordered: true, .. }))
        .count();
    assert_eq!(count, 2, "got {:?}", tokens);
}

#[test]
fn paren_marker_preserves_number() {
    let tokens = parse("5) five");
    if let Token::ListItem { number, ordered, .. } = &tokens[0] {
        assert!(*ordered);
        assert_eq!(*number, Some(5));
    } else {
        panic!("expected ordered list item, got {:?}", tokens);
    }
}

#[test]
fn dot_marker_still_works() {
    let tokens = parse("1. one");
    if let Token::ListItem { ordered, number, .. } = &tokens[0] {
        assert!(*ordered);
        assert_eq!(*number, Some(1));
    } else {
        panic!("expected ordered list item, got {:?}", tokens);
    }
}
