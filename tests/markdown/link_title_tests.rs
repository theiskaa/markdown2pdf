use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn link_with_double_quote_title_strips_title_from_url() {
    let tokens = parse(r#"[text](url "title here")"#);
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("text".to_string())],
            url: "url".to_string(),
            title: Some("title here".to_string()),
        }]
    );
}

#[test]
fn link_with_single_quote_title() {
    let tokens = parse("[text](url 'title here')");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("text".to_string())],
            url: "url".to_string(),
            title: Some("title here".to_string()),
        }]
    );
}

#[test]
fn link_with_paren_title() {
    let tokens = parse("[text](url (title here))");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("text".to_string())],
            url: "url".to_string(),
            title: Some("title here".to_string()),
        }]
    );
}

#[test]
fn image_with_title() {
    let tokens = parse(r#"![alt](pic.png "Photo of cat")"#);
    assert_eq!(
        tokens,
        vec![Token::Image {
            alt: vec![Token::Text("alt".to_string())],
            url: "pic.png".to_string(),
            title: Some("Photo of cat".to_string()),
        }]
    );
}

#[test]
fn link_no_title_unchanged() {
    let tokens = parse("[text](url)");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("text".to_string())],
            url: "url".to_string(),
            title: None,
        }]
    );
}

#[test]
fn link_url_paren_pair_with_title() {
    // URL contains balanced parens AND a title at the end.
    let tokens = parse(r#"[Wiki](https://en.wikipedia.org/wiki/Foo_(bar) "Wikipedia entry")"#);
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("Wiki".to_string())],
            url: "https://en.wikipedia.org/wiki/Foo_(bar)".to_string(),
            title: Some("Wikipedia entry".to_string()),
        }]
    );
}

#[test]
fn link_with_only_whitespace_after_url_no_title() {
    // Trailing whitespace before `)` without a title is fine.
    let tokens = parse("[text](url   )");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("text".to_string())],
            url: "url".to_string(),
            title: None
        }]
    );
}

#[test]
fn link_url_with_no_space_then_quote_is_url_only() {
    // `(url"foo")` with no whitespace between url and quote — not a title.
    // The whole `url"foo"` is the URL.
    let tokens = parse("[text](url\"foo\")");
    if let Token::Link { url, .. } = &tokens[0] {
        assert!(
            url.contains("\""),
            "expected url to contain quote, got {:?}",
            url
        );
    } else {
        panic!("expected link, got {:?}", tokens);
    }
}
