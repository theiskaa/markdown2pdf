//! Blockquote corner cases. Targets `parse_blockquote`,
//! `line_starts_new_block_at`.

use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn basic_blockquote() {
    let tokens = parse("> quote\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::BlockQuote(_))));
}

#[test]
fn blockquote_without_space_after_marker() {
    let tokens = parse(">no space\n");
    let Some(Token::BlockQuote(body)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    assert!(Token::collect_all_text(body).contains("no space"));
}

#[test]
fn lazy_continuation() {
    let tokens = parse("> first\nlazy continuation\n");
    let Some(Token::BlockQuote(body)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    let text = Token::collect_all_text(body);
    assert!(text.contains("first"));
    assert!(text.contains("lazy continuation"));
}

#[test]
fn nested_blockquote() {
    let tokens = parse("> > deeper\n");
    let Some(Token::BlockQuote(outer)) = tokens.first() else {
        panic!("expected outer BlockQuote, got {:?}", tokens);
    };
    assert!(outer.iter().any(|t| matches!(t, Token::BlockQuote(_))));
}

#[test]
fn nested_three_deep() {
    let tokens = parse("> > > deeper\n");
    let Some(Token::BlockQuote(outer)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    let mut current = outer;
    for _ in 0..2 {
        let nested = current.iter().find_map(|t| {
            if let Token::BlockQuote(inner) = t { Some(inner) } else { None }
        });
        let Some(inner) = nested else {
            panic!("expected nested BlockQuote at this level");
        };
        current = inner;
    }
}

#[test]
fn blockquote_containing_fenced_code() {
    let tokens = parse("> ```\n> code line\n> ```\n");
    let Some(Token::BlockQuote(body)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    assert!(body.iter().any(|t| matches!(t, Token::Code { block: true, .. })));
}

#[test]
fn blockquote_with_indent_up_to_three() {
    for indent in 0..=3 {
        let input = format!("{}> q\n", " ".repeat(indent));
        assert!(parse(&input).iter().any(|t| matches!(t, Token::BlockQuote(_))));
    }
}
