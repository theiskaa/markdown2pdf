use markdown2pdf::markdown::*;

use super::common::parse;


fn block_body(t: &Token) -> &Vec<Token> {
    if let Token::BlockQuote(body) = t {
        body
    } else {
        panic!("expected BlockQuote, got {:?}", t);
    }
}

#[test]
fn setext_h2_inside_blockquote() {
    let tokens = parse("> Title\n> ---");
    let body = block_body(&tokens[0]);
    assert!(
        body.iter().any(|t| matches!(t, Token::Heading(_, 2))),
        "expected H2 inside quote, got {:?}",
        body
    );
}

#[test]
fn setext_h1_inside_blockquote() {
    let tokens = parse("> Big\n> ===");
    let body = block_body(&tokens[0]);
    assert!(
        body.iter().any(|t| matches!(t, Token::Heading(_, 1))),
        "expected H1 inside quote, got {:?}",
        body
    );
}

#[test]
fn indented_code_inside_blockquote() {
    let tokens = parse(">     code line in quote");
    let body = block_body(&tokens[0]);
    assert!(
        body.iter().any(|t| matches!(t, Token::Code { .. })),
        "expected Code inside quote, got {:?}",
        body
    );
}

#[test]
fn regular_text_inside_blockquote_unaffected() {
    let tokens = parse("> Just a sentence with three spaces:    not code.");
    let body = block_body(&tokens[0]);
    assert!(!body.iter().any(|t| matches!(t, Token::Code { .. })));
}
