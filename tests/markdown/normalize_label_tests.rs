//! Direct tests for `normalize_label` and `case_fold_char`. These pin the
//! current behavior, which approximates Unicode full case-folding via
//! `to_lowercase` plus a handful of special cases (`ß` / `ẞ` → `ss`,
//! `İ` → `i\u{0307}`, etc.). Tests here use the public reference-link
//! resolution as the integration surface.

use markdown2pdf::markdown::*;

use super::common::parse;

fn link_url_of(input: &str) -> String {
    let tokens = parse(input);
    let Some(Token::Link { url, .. }) = tokens.iter().find(|t| matches!(t, Token::Link { .. }))
    else {
        panic!("expected a link in {:?}", tokens);
    };
    url.clone()
}

#[test]
fn ascii_case_fold() {
    assert_eq!(link_url_of("[FOO]\n\n[foo]: /a\n"), "/a");
    assert_eq!(link_url_of("[foo]\n\n[FOO]: /a\n"), "/a");
}

#[test]
fn internal_whitespace_collapsed() {
    assert_eq!(link_url_of("[foo   bar]\n\n[foo bar]: /a\n"), "/a");
}

#[test]
fn leading_trailing_whitespace_trimmed_via_collapse() {
    // The `[ foo ]` label normalizes to `foo` (single internal space then
    // trim — but here there's no internal text so just trimmed).
    assert_eq!(link_url_of("[  foo  ]\n\n[foo]: /a\n"), "/a");
}

#[test]
fn tab_folds_to_single_space() {
    assert_eq!(link_url_of("[a\tb]\n\n[a b]: /a\n"), "/a");
}

#[test]
fn newline_folds_to_single_space() {
    // Reference labels may span lines.
    assert_eq!(link_url_of("[a\nb]\n\n[a b]: /a\n"), "/a");
}

#[test]
fn sharp_s_folds_to_ss() {
    // `ß` ↔ `ss` per the `case_fold_char` special case.
    assert_eq!(link_url_of("[Straße]\n\n[strasse]: /a\n"), "/a");
}

#[test]
fn capital_sharp_s_folds_to_ss() {
    assert_eq!(link_url_of("[GROẞ]\n\n[gross]: /a\n"), "/a");
}

#[test]
fn cjk_left_alone() {
    // CJK has no case — both labels must match byte-for-byte after
    // whitespace collapse.
    assert_eq!(link_url_of("[日本語]\n\n[日本語]: /a\n"), "/a");
}

#[test]
fn mixed_case_with_unicode_punctuation() {
    assert_eq!(
        link_url_of("[Hello, World!]\n\n[hello, world!]: /a\n"),
        "/a"
    );
}

#[test]
fn label_with_only_internal_spacing_difference() {
    // Different surrounding whitespace, same collapsed label.
    assert_eq!(link_url_of("[a  b  c]\n\n[a b c]: /a\n"), "/a");
}
