use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn unchecked_task_list_item() {
    let tokens = parse("- [ ] Pending task");
    if let Token::ListItem {
        content, checked, ..
    } = &tokens[0]
    {
        assert_eq!(*checked, Some(false), "expected unchecked");
        let text = Token::collect_all_text(content);
        assert!(text.contains("Pending task"), "got {:?}", text);
    } else {
        panic!("expected list item, got {:?}", tokens);
    }
}

#[test]
fn checked_task_list_item() {
    let tokens = parse("- [x] Done task");
    if let Token::ListItem {
        content, checked, ..
    } = &tokens[0]
    {
        assert_eq!(*checked, Some(true), "expected checked");
        let text = Token::collect_all_text(content);
        assert!(text.contains("Done task"), "got {:?}", text);
    } else {
        panic!("expected list item, got {:?}", tokens);
    }
}

#[test]
fn task_list_capital_x() {
    let tokens = parse("- [X] also done");
    if let Token::ListItem { checked, .. } = &tokens[0] {
        assert_eq!(*checked, Some(true));
    } else {
        panic!("expected list item, got {:?}", tokens);
    }
}

#[test]
fn regular_list_item_has_no_checkbox() {
    let tokens = parse("- regular item");
    if let Token::ListItem { checked, .. } = &tokens[0] {
        assert_eq!(*checked, None);
    } else {
        panic!("expected list item, got {:?}", tokens);
    }
}

#[test]
fn ordered_task_list_item() {
    // GFM allows task markers on ordered lists too.
    let tokens = parse("1. [ ] First task");
    if let Token::ListItem {
        content,
        checked,
        ordered,
        number,
        marker: _,
        loose: _,
    } = &tokens[0]
    {
        assert!(ordered);
        assert_eq!(*number, Some(1));
        assert_eq!(*checked, Some(false));
        assert!(Token::collect_all_text(content).contains("First task"));
    } else {
        panic!("expected list item, got {:?}", tokens);
    }
}

#[test]
fn tilde_fenced_code_block_basic() {
    let input = "~~~\nfn main() {}\n~~~";
    let tokens = parse(input);
    assert_eq!(
        tokens,
        vec![Token::Code {
            language: "".to_string(),
            content: "fn main() {}".to_string(),
            block: true,
        }]
    );
}

#[test]
fn tilde_fenced_code_block_with_language() {
    let input = "~~~rust\nlet x = 5;\n~~~";
    let tokens = parse(input);
    assert_eq!(
        tokens,
        vec![Token::Code {
            language: "rust".to_string(),
            content: "let x = 5;".to_string(),
            block: true
        }]
    );
}

#[test]
fn tilde_fence_can_contain_backticks() {
    // The whole point of `~~~` is letting code contain literal backticks.
    let input = "~~~\nlet s = `template`;\n~~~";
    let tokens = parse(input);
    if let Token::Code { content: body, .. } = &tokens[0] {
        assert!(body.contains("`template`"), "got {:?}", body);
    } else {
        panic!("expected code, got {:?}", tokens);
    }
}

#[test]
fn strikethrough_basic() {
    let tokens = parse("~~deleted~~");
    assert!(
        tokens.iter().any(|t| matches!(t, Token::Strikethrough(_))),
        "expected Strikethrough, got {:?}",
        tokens
    );
    if let Token::Strikethrough(content) = &tokens[0] {
        assert_eq!(Token::collect_all_text(content), "deleted");
    }
}

#[test]
fn strikethrough_inside_paragraph() {
    let tokens = parse("This is ~~old~~ news.");
    assert!(tokens.iter().any(|t| matches!(t, Token::Strikethrough(_))));
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("old"), "got {:?}", text);
    assert!(text.contains("news"), "got {:?}", text);
}

#[test]
fn strikethrough_unmatched_falls_back() {
    // An unmatched ~~ must not abort — it falls back to literal text.
    let tokens = parse("starts ~~ but never closes");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("~~"), "got {:?}", text);
}

#[test]
fn single_tilde_is_not_strikethrough() {
    // Only ~~ (two or more) opens strikethrough; lone ~ is plain text.
    let tokens = parse("a ~ b");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Strikethrough(_))));
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("~"), "got {:?}", text);
}

#[test]
fn strikethrough_with_emphasis_inside() {
    let tokens = parse("~~deleted *and italic*~~");
    if let Token::Strikethrough(content) = &tokens[0] {
        assert!(content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    } else {
        panic!("expected Strikethrough, got {:?}", tokens);
    }
}

#[test]
fn tilde_in_inline_code_stays_literal() {
    let tokens = parse("`~~not strikethrough~~`");
    assert_eq!(
        tokens,
        vec![Token::Code {
            language: "".to_string(),
            content: "~~not strikethrough~~".to_string(),
            block: false
        }]
    );
}
