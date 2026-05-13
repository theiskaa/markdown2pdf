use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn escape_close_bracket_in_link_text() {
    let tokens = parse(r"[a\]b](http://x)");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("a]b".to_string())], url: "http://x".to_string(), title: None }]
    );
}

#[test]
fn escape_close_paren_in_link_url() {
    let tokens = parse(r"[t](http://x\)y)");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("t".to_string())], url: "http://x)y".to_string(), title: None }]
    );
}

#[test]
fn escape_backslash_in_link_text() {
    let tokens = parse(r"[a\\b](u)");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("a\\b".to_string())], url: "u".to_string(), title: None }]
    );
}

#[test]
fn escape_close_bracket_in_image_alt() {
    let tokens = parse(r"![alt\]more](pic.png)");
    assert_eq!(
        tokens,
        vec![Token::Image { alt: vec![Token::Text("alt]more".to_string())], url: "pic.png".to_string(), title: None }]
    );
}

#[test]
fn unescaped_link_still_works() {
    let tokens = parse("[foo](http://example.com)");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("foo".to_string())], url: "http://example.com".to_string(), title: None }]
    );
}

#[test]
fn balanced_parens_still_work() {
    // Pre-existing balanced-paren handling shouldn't regress.
    let tokens = parse("[Wiki](https://en.wikipedia.org/wiki/Foo_(bar))");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("Wiki".to_string())], url: "https://en.wikipedia.org/wiki/Foo_(bar)".to_string(), title: None }]
    );
}
