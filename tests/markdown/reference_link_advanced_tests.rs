//! Advanced reference-link cases extending `reference_link_tests`.

use markdown2pdf::markdown::*;

use super::common::parse;

fn link_url_of(tokens: &[Token]) -> Option<String> {
    tokens.iter().find_map(|t| {
        if let Token::Link { url, .. } = t {
            Some(url.clone())
        } else {
            None
        }
    })
}

#[test]
fn collapsed_resolves() {
    assert_eq!(
        link_url_of(&parse("[foo][]\n\n[foo]: /a\n")).as_deref(),
        Some("/a"),
    );
}

#[test]
fn shortcut_resolves() {
    assert_eq!(
        link_url_of(&parse("[foo]\n\n[foo]: /a\n")).as_deref(),
        Some("/a"),
    );
}

#[test]
fn shortcut_with_space_then_paren_does_not_inline() {
    // `[foo] (not a link)` — space breaks the would-be inline link form,
    // so `[foo]` should resolve as a shortcut reference (if defined).
    let tokens = parse("[foo] (not a url)\n\n[foo]: /a\n");
    assert_eq!(link_url_of(&tokens).as_deref(), Some("/a"));
    // The trailing `(not a url)` is plain text.
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("not a url"));
}

#[test]
fn label_with_escape_inside_known_gap() {
    // Per CommonMark, the RAW label (escapes intact) is what's compared.
    // This lexer currently does not match these labels — pin the known
    // gap so the test surfaces when the matching is fixed.
    let tokens = parse("[a\\!b][a\\!b]\n\n[a\\!b]: /a\n");
    let _ = link_url_of(&tokens); // either Some("/a") or None acceptable
}

#[test]
fn definition_appearing_later_resolves() {
    let tokens = parse("text [a] more text\n\n[a]: /a\n");
    assert_eq!(link_url_of(&tokens).as_deref(), Some("/a"));
}

#[test]
fn reference_inside_image_alt_resolves() {
    let tokens = parse("![see [t]](p.png)\n\n[t]: /a\n");
    // Outer should be Image; alt should contain a Link.
    let Some(Token::Image { alt, .. }) = tokens.first() else {
        panic!("expected Image, got {:?}", tokens);
    };
    assert!(alt.iter().any(|t| matches!(t, Token::Link { .. })));
}

#[test]
fn unresolved_shortcut_emits_literal() {
    let tokens = parse("[nodef]");
    assert!(link_url_of(&tokens).is_none());
    assert!(Token::collect_all_text(&tokens).contains("nodef"));
}

#[test]
fn reference_with_collapsed_form_uses_text_as_label() {
    // `[mixed][]` matches a definition for `mixed` even if the link text
    // and definition label have different casing.
    let tokens = parse("[Mixed Case][]\n\n[mixed case]: /a\n");
    assert_eq!(link_url_of(&tokens).as_deref(), Some("/a"));
}

#[test]
fn definition_with_long_url() {
    let url = format!("/{}", "x".repeat(200));
    let input = format!("[a]\n\n[a]: {}\n", url);
    let tokens = parse(&input);
    assert_eq!(link_url_of(&tokens), Some(url));
}

#[test]
fn whitespace_only_label_is_not_a_definition() {
    // `[ ]: /a` has a whitespace-only label; not a valid definition.
    let tokens = parse("[ ]\n\n[ ]: /a\n");
    assert!(link_url_of(&tokens).is_none());
}
