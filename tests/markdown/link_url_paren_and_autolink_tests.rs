use markdown2pdf::markdown::*;

use super::common::parse;



#[test]
fn url_with_single_balanced_paren_pair() {
    let tokens = parse("[Wiki](https://en.wikipedia.org/wiki/Foo_(bar))");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("Wiki".to_string())], url: "https://en.wikipedia.org/wiki/Foo_(bar)".to_string(), title: None }]
    );
}

#[test]
fn url_with_nested_balanced_parens() {
    let tokens = parse("[X](http://a.b/((c)d))");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("X".to_string())], url: "http://a.b/((c)d)".to_string(), title: None }]
    );
}

#[test]
fn image_url_with_paren_pair() {
    let tokens = parse("![alt](pic_(small).png)");
    assert_eq!(
        tokens,
        vec![Token::Image { alt: vec![Token::Text("alt".to_string())], url: "pic_(small).png".to_string(), title: None }]
    );
}

#[test]
fn url_with_unbalanced_close_paren_truncates() {
    let tokens = parse("[X](https://example.com/path)trailing");
    if let Token::Link { content, url, .. } = &tokens[0] {
        assert_eq!(Token::collect_all_text(content), "X");
        assert_eq!(url, "https://example.com/path");
    } else {
        panic!("expected link, got {:?}", tokens);
    }
}


#[test]
fn autolink_https() {
    let tokens = parse("<https://example.com>");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("https://example.com".to_string())], url: "https://example.com".to_string(), title: None }]
    );
}

#[test]
fn autolink_http() {
    let tokens = parse("<http://example.org/path>");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("http://example.org/path".to_string())], url: "http://example.org/path".to_string(), title: None }]
    );
}

#[test]
fn autolink_email() {
    let tokens = parse("<user@example.com>");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("user@example.com".to_string())], url: "mailto:user@example.com".to_string(), title: None }]
    );
}

#[test]
fn autolink_in_paragraph() {
    let tokens = parse("see <https://example.com> for more");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Link { url, .. } if url == "https://example.com")),
        "got {:?}",
        tokens
    );
}

#[test]
fn invalid_autolink_falls_through_as_text() {
    let tokens = parse("<not an autolink>");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("<not an autolink>"), "got {:?}", text);
}


#[test]
fn html_comment_still_parsed() {
    // `<!--` at line start is now a block-level HtmlBlock (CommonMark
    // §4.6 type 2). The inline HtmlComment variant fires only when the
    // comment is preceded by other content on the same line — that's
    // tested in parse_html_comment_tests.rs.
    let tokens = parse("<!-- comment -->");
    assert!(matches!(tokens[0], Token::HtmlBlock(_)));
}

#[test]
fn regression_simple_link() {
    let tokens = parse("[example](https://example.com)");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("example".to_string())], url: "https://example.com".to_string(), title: None }]
    );
}

#[test]
fn regression_simple_image() {
    let tokens = parse("![alt](image.png)");
    assert_eq!(
        tokens,
        vec![Token::Image { alt: vec![Token::Text("alt".to_string())], url: "image.png".to_string(), title: None }]
    );
}
