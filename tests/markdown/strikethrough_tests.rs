use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn basic_double_tilde() {
    let tokens = parse("~~gone~~");
    assert!(
        tokens.iter().any(|t| matches!(t, Token::Strikethrough(_))),
        "expected Strikethrough, got {:?}",
        tokens
    );
}

#[test]
fn single_tilde_is_literal() {
    let tokens = parse("a ~ b");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Strikethrough(_))));
    assert!(Token::collect_all_text(&tokens).contains('~'));
}

#[test]
fn unmatched_double_tilde_falls_back_to_text() {
    let tokens = parse("~~never closes");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Strikethrough(_))));
    assert!(Token::collect_all_text(&tokens).contains("~~"));
}

#[test]
fn strikethrough_with_emphasis_inside() {
    let tokens = parse("~~gone *italic*~~");
    let Token::Strikethrough(content) = &tokens[0] else {
        panic!("expected Strikethrough, got {:?}", tokens);
    };
    assert!(content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn strikethrough_with_code_inside() {
    let tokens = parse("~~`code`~~");
    let Token::Strikethrough(content) = &tokens[0] else {
        panic!("expected Strikethrough, got {:?}", tokens);
    };
    assert!(content.iter().any(|t| matches!(t, Token::Code { block: false, .. })));
}

#[test]
fn strikethrough_with_link_inside() {
    let tokens = parse("~~[t](u)~~");
    let Token::Strikethrough(content) = &tokens[0] else {
        panic!("expected Strikethrough, got {:?}", tokens);
    };
    assert!(content.iter().any(|t| matches!(t, Token::Link { .. })));
}

#[test]
fn tilde_inside_inline_code_is_literal() {
    let tokens = parse("`~~not~~`");
    assert_eq!(
        tokens,
        vec![Token::Code {
            language: "".to_string(),
            content: "~~not~~".to_string(),
            block: false,
        }]
    );
}

#[test]
fn emphasis_wrapping_strikethrough() {
    let tokens = parse("*one ~~two~~ three*");
    let Token::Emphasis { content, .. } = &tokens[0] else {
        panic!("expected Emphasis, got {:?}", tokens);
    };
    assert!(content.iter().any(|t| matches!(t, Token::Strikethrough(_))));
}

#[test]
fn strikethrough_inside_blockquote() {
    let tokens = parse("> ~~gone~~");
    let Token::BlockQuote(body) = &tokens[0] else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    assert!(body.iter().any(|t| matches!(t, Token::Strikethrough(_))));
}

#[test]
fn strikethrough_inside_list_item() {
    let tokens = parse("- ~~gone~~\n");
    let Token::ListItem { content, .. } = &tokens[0] else {
        panic!("expected ListItem, got {:?}", tokens);
    };
    assert!(content.iter().any(|t| matches!(t, Token::Strikethrough(_))));
}

#[test]
fn strikethrough_inside_heading() {
    let tokens = parse("# ~~gone~~\n");
    let Token::Heading(body, _) = &tokens[0] else {
        panic!("expected Heading, got {:?}", tokens);
    };
    assert!(body.iter().any(|t| matches!(t, Token::Strikethrough(_))));
}
