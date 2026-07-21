use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn entity_in_link_text_decodes() {
    let tokens = parse("[a &amp; b](http://x.com)");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("a & b".to_string())],
            url: "http://x.com".to_string(),
            title: None
        }]
    );
}

#[test]
fn numeric_entity_in_link_text_decodes() {
    let tokens = parse("[em &#8212; dash](u)");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("em — dash".to_string())],
            url: "u".to_string(),
            title: None
        }]
    );
}

#[test]
fn entity_in_link_url_decodes() {
    let tokens = parse("[link](http://example.com/?a=1&amp;b=2)");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("link".to_string())],
            url: "http://example.com/?a=1&b=2".to_string(),
            title: None
        }]
    );
}

#[test]
fn numeric_entity_in_link_url_decodes() {
    let tokens = parse("[t](http://x/&#35;frag)");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("t".to_string())],
            url: "http://x/#frag".to_string(),
            title: None
        }]
    );
}

#[test]
fn entity_in_image_alt_decodes() {
    let tokens = parse("![an &amp; alt](pic.png)");
    assert_eq!(
        tokens,
        vec![Token::Image {
            alt: vec![Token::Text("an & alt".to_string())],
            url: "pic.png".to_string(),
            title: None
        }]
    );
}

#[test]
fn entity_in_image_url_decodes() {
    let tokens = parse("![alt](http://x/?q=1&amp;y=2)");
    assert_eq!(
        tokens,
        vec![Token::Image {
            alt: vec![Token::Text("alt".to_string())],
            url: "http://x/?q=1&y=2".to_string(),
            title: None
        }]
    );
}

#[test]
fn unknown_entity_in_link_text_passes_through() {
    let tokens = parse("[&zzz;](u)");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("&zzz;".to_string())],
            url: "u".to_string(),
            title: None
        }]
    );
}

#[test]
fn lone_ampersand_in_link_text_stays_literal() {
    let tokens = parse("[a & b](u)");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("a & b".to_string())],
            url: "u".to_string(),
            title: None
        }]
    );
}

#[test]
fn entity_inside_escape_in_link_text() {
    // Escape applies first, entity decoding still works for unescaped chars.
    let tokens = parse(r"[\[ &amp; \]](u)");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("[ & ]".to_string())],
            url: "u".to_string(),
            title: None
        }]
    );
}

#[test]
fn autolink_url_does_not_decode_entities() {
    // autolink URLs are literal — entities preserved verbatim.
    let tokens = parse("<http://x.com/?a=&amp;b>");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("http://x.com/?a=&amp;b".to_string())],
            url: "http://x.com/?a=&amp;b".to_string(),
            title: None
        }]
    );
}

#[test]
fn reference_label_with_entity_does_not_resolve() {
    // Per CommonMark, link-label comparison is on RAW source chars
    // (case-folded, whitespace-collapsed). Entity and backslash escapes
    // are NOT decoded before matching — `caf&eacute;` doesn't match a
    // `[café]` definition.
    let tokens = parse("[link][caf&eacute;]\n\n[café]: /u");
    let resolved = tokens.iter().any(|t| {
        matches!(
            t,
            Token::Link { content, url, .. }
            if Token::collect_all_text(content) == "link" && url == "/u"
        )
    });
    assert!(
        !resolved,
        "reference label with entity must not resolve to a literal-char def; got {}",
        Token::slice_to_compact(&tokens)
    );
}
