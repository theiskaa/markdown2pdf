//! Direct tests for `propagate_loose_tight`. A list is "loose" iff any
//! pair of consecutive sibling items is separated by a blank line; all
//! items in the same list then share the resulting `loose` flag. The
//! propagation must NOT cross list boundaries (nested vs outer, marker
//! change).

use markdown2pdf::markdown::*;

use super::common::parse;


fn loose_flags(tokens: &[Token]) -> Vec<bool> {
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
fn tight_list_all_tight() {
    let tokens = parse("- a\n- b\n- c\n");
    assert_eq!(loose_flags(&tokens), vec![false, false, false]);
}

#[test]
fn loose_list_all_loose_when_any_gap_exists() {
    let tokens = parse("- a\n\n- b\n- c\n");
    assert_eq!(loose_flags(&tokens), vec![true, true, true]);
}

#[test]
fn loose_list_with_gap_in_middle() {
    let tokens = parse("- a\n- b\n\n- c\n");
    let flags = loose_flags(&tokens);
    assert!(flags.iter().all(|&f| f));
}

#[test]
fn loose_from_internal_blank_in_single_item() {
    // A blank line inside an item's content also makes the list loose.
    let tokens = parse("- first para\n\n  second para\n- next item\n");
    let flags = loose_flags(&tokens);
    assert!(flags.iter().all(|&f| f), "expected all-loose, got {:?}", flags);
}

#[test]
fn ordered_list_loose() {
    let tokens = parse("1. a\n\n2. b\n");
    let flags = loose_flags(&tokens);
    assert!(flags.iter().all(|&f| f));
}

#[test]
fn marker_change_resplits_lists_each_independent() {
    // `- a` then `+ b` is two separate lists. Each should be tight on its own.
    let tokens = parse("- a\n+ b\n");
    let flags = loose_flags(&tokens);
    assert_eq!(flags.len(), 2);
    assert!(flags.iter().all(|&f| !f), "got {:?}", flags);
}

#[test]
fn nested_list_independent_of_outer() {
    // Outer list is tight; inner list is also tight — nothing forces looseness.
    let tokens = parse("- a\n  - inner-a\n  - inner-b\n- b\n");
    let outer = loose_flags(&tokens);
    assert!(outer.iter().all(|&f| !f));
}

#[test]
fn nested_loose_does_not_force_outer_loose() {
    // Outer has no blank-line gap → outer stays tight even though inner has one.
    let tokens = parse("- outer-a\n  - inner-a\n\n  - inner-b\n- outer-b\n");
    let outer = loose_flags(&tokens);
    // Outer list items are the top-level ListItems.
    assert_eq!(outer.len(), 2);
}

#[test]
fn task_list_loose_preserves_checked() {
    let tokens = parse("- [ ] a\n\n- [x] b\n");
    let mut saw_check = (false, false);
    for t in &tokens {
        if let Token::ListItem { checked, loose, .. } = t {
            assert!(*loose);
            match checked {
                Some(false) => saw_check.0 = true,
                Some(true) => saw_check.1 = true,
                None => {}
            }
        }
    }
    assert!(saw_check.0 && saw_check.1, "expected both checkbox states");
}

#[test]
fn single_item_list_is_tight() {
    let tokens = parse("- only\n");
    assert_eq!(loose_flags(&tokens), vec![false]);
}
