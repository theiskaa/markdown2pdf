//! Direct tests for `parse_image` — covering inline / reference / collapsed
//! / shortcut forms, alt-text inline parsing, and the bare-`!` fallback.

use markdown2pdf::markdown::*;

use super::common::parse;


fn first_image(tokens: &[Token]) -> (&Vec<Token>, &str, &Option<String>) {
    let Some(Token::Image { alt, url, title }) = tokens.iter().find(|t| matches!(t, Token::Image { .. })) else {
        panic!("expected Image, got {:?}", tokens);
    };
    (alt, url.as_str(), title)
}

#[test]
fn inline_image_basic() {
    let tokens = parse("![alt](pic.png)");
    let (alt, url, title) = first_image(&tokens);
    assert_eq!(Token::collect_all_text(alt), "alt");
    assert_eq!(url, "pic.png");
    assert!(title.is_none());
}

#[test]
fn inline_image_with_double_quoted_title() {
    let tokens = parse(r#"![alt](pic.png "caption")"#);
    let (_, _, title) = first_image(&tokens);
    assert_eq!(title.as_deref(), Some("caption"));
}

#[test]
fn inline_image_with_single_quoted_title() {
    let tokens = parse(r#"![alt](pic.png 'caption')"#);
    let (_, _, title) = first_image(&tokens);
    assert_eq!(title.as_deref(), Some("caption"));
}

#[test]
fn inline_image_with_paren_title() {
    let tokens = parse(r#"![alt](pic.png (caption))"#);
    let (_, _, title) = first_image(&tokens);
    assert_eq!(title.as_deref(), Some("caption"));
}

#[test]
fn inline_image_alt_with_emphasis() {
    let tokens = parse("![*it* matters](p.png)");
    let (alt, _, _) = first_image(&tokens);
    assert!(alt.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn inline_image_alt_with_code_span() {
    let tokens = parse("![`code` shot](p.png)");
    let (alt, _, _) = first_image(&tokens);
    assert!(alt.iter().any(|t| matches!(t, Token::Code { block: false, .. })));
}

#[test]
fn inline_image_alt_with_entity_decoded() {
    let tokens = parse("![a &amp; b](p.png)");
    let (alt, _, _) = first_image(&tokens);
    assert_eq!(Token::collect_all_text(alt), "a & b");
}

#[test]
fn inline_image_alt_with_backslash_escape() {
    let tokens = parse(r"![a\*b\*c](p.png)");
    let (alt, _, _) = first_image(&tokens);
    // The escape should prevent emphasis.
    assert!(!alt.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    assert_eq!(Token::collect_all_text(alt), "a*b*c");
}

#[test]
fn inline_image_with_nested_link_in_alt() {
    // Image alt can contain a link (rendered to plain text).
    let tokens = parse("![see [text](u)](p.png)");
    let (_, url, _) = first_image(&tokens);
    assert_eq!(url, "p.png");
}

#[test]
fn inline_image_empty_alt() {
    let tokens = parse("![](p.png)");
    let (alt, url, _) = first_image(&tokens);
    assert_eq!(Token::collect_all_text(alt), "");
    assert_eq!(url, "p.png");
}

#[test]
fn inline_image_empty_url() {
    let tokens = parse("![alt]()");
    let (_, url, _) = first_image(&tokens);
    assert_eq!(url, "");
}

#[test]
fn bare_exclamation_is_literal() {
    let tokens = parse("hello! world");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Image { .. })));
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("hello!"));
    assert!(text.contains("world"));
}

#[test]
fn exclamation_then_bracket_no_closer_is_literal() {
    // `![alt` without closing `]` falls back to text.
    let tokens = parse("![never closes");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Image { .. })));
}

#[test]
fn reference_image_full() {
    let tokens = parse("![alt][lab]\n\n[lab]: u \"t\"\n");
    let (alt, url, title) = first_image(&tokens);
    assert_eq!(Token::collect_all_text(alt), "alt");
    assert_eq!(url, "u");
    assert_eq!(title.as_deref(), Some("t"));
}

#[test]
fn reference_image_collapsed() {
    let tokens = parse("![alt][]\n\n[alt]: u\n");
    let (_, url, _) = first_image(&tokens);
    assert_eq!(url, "u");
}

#[test]
fn reference_image_shortcut() {
    let tokens = parse("![alt]\n\n[alt]: u\n");
    let (_, url, _) = first_image(&tokens);
    assert_eq!(url, "u");
}

#[test]
fn reference_image_label_case_folded() {
    let tokens = parse("![ALT]\n\n[alt]: u\n");
    let (_, url, _) = first_image(&tokens);
    assert_eq!(url, "u");
}

#[test]
fn unresolved_reference_image_falls_back_to_text() {
    let tokens = parse("![alt][missing]");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Image { .. })));
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("alt"));
    assert!(text.contains("missing"));
}

#[test]
fn unresolved_shortcut_image_falls_back_to_text() {
    let tokens = parse("![nodef]");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Image { .. })));
    assert!(Token::collect_all_text(&tokens).contains("nodef"));
}

#[test]
fn image_inside_link_text_yields_link_wrapping_image() {
    let tokens = parse("[![inner](i.png)](outer)");
    let Some(Token::Link { content, .. }) = tokens.first() else {
        panic!("expected outer Link, got {:?}", tokens);
    };
    assert!(content.iter().any(|t| matches!(t, Token::Image { .. })));
}

#[test]
fn image_alt_text_contains_image_url_only_in_title() {
    let tokens = parse(r#"![alt](u.png "the title")"#);
    let (alt, url, title) = first_image(&tokens);
    let alt_text = Token::collect_all_text(alt);
    assert!(!alt_text.contains("u.png"));
    assert!(!alt_text.contains("the title"));
    assert_eq!(url, "u.png");
    assert_eq!(title.as_deref(), Some("the title"));
}

#[test]
fn reference_image_label_with_whitespace_collapsed() {
    let tokens = parse("![multi  word]\n\n[multi word]: u\n");
    let (_, url, _) = first_image(&tokens);
    assert_eq!(url, "u");
}
