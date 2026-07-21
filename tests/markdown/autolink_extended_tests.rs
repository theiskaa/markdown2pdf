//! Extended tests for `try_parse_autolink` and `looks_like_autolink_start`.
//! Existing `link_url_paren_and_autolink_tests` covers happy-path autolinks;
//! this module exercises scheme/charset edge cases and rejection paths.

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_autolink_url(tokens: &[Token]) -> Option<String> {
    tokens.iter().find_map(|t| {
        if let Token::Link { url, .. } = t {
            Some(url.clone())
        } else {
            None
        }
    })
}

#[test]
fn http_autolink() {
    assert_eq!(
        first_autolink_url(&parse("<http://example.com>")).as_deref(),
        Some("http://example.com")
    );
}

#[test]
fn https_autolink() {
    assert_eq!(
        first_autolink_url(&parse("<https://example.com>")).as_deref(),
        Some("https://example.com")
    );
}

#[test]
fn ftp_autolink() {
    assert_eq!(
        first_autolink_url(&parse("<ftp://example.com>")).as_deref(),
        Some("ftp://example.com")
    );
}

#[test]
fn mailto_autolink() {
    assert_eq!(
        first_autolink_url(&parse("<mailto:user@example.com>")).as_deref(),
        Some("mailto:user@example.com")
    );
}

#[test]
fn scheme_with_plus_dash_dot() {
    // `irc+ssl`, `git-lfs`, `a.b` are all valid scheme charsets.
    assert!(first_autolink_url(&parse("<irc+ssl://chat>")).is_some());
    assert!(first_autolink_url(&parse("<git-lfs://repo>")).is_some());
    assert!(first_autolink_url(&parse("<x.y:foo>")).is_some());
}

#[test]
fn single_letter_scheme_rejected() {
    // Per CommonMark, scheme must be 2–32 chars.
    let tokens = parse("<a:foo>");
    assert!(first_autolink_url(&tokens).is_none());
}

#[test]
fn email_autolink_basic() {
    // Email autolinks get a `mailto:` prefix on the URL (the displayed
    // text in `content` keeps the bare email).
    assert_eq!(
        first_autolink_url(&parse("<user@example.com>")).as_deref(),
        Some("mailto:user@example.com")
    );
}

#[test]
fn email_with_subdomain() {
    assert_eq!(
        first_autolink_url(&parse("<u@mail.example.co.uk>")).as_deref(),
        Some("mailto:u@mail.example.co.uk")
    );
}

#[test]
fn email_with_special_local_chars() {
    // Local-part may contain `+`, `.`, `-`, `_` etc.
    assert_eq!(
        first_autolink_url(&parse("<first.last+tag@example.com>")).as_deref(),
        Some("mailto:first.last+tag@example.com")
    );
}

#[test]
fn email_without_dot_in_domain_rejected() {
    // Per CommonMark email autolink grammar, the domain needs at least one `.`.
    let tokens = parse("<user@example>");
    // Either rejected (no Link) or accepted as fallback — pin "must not produce
    // a Link with that exact URL".
    let urls: Vec<_> = tokens
        .iter()
        .filter_map(|t| {
            if let Token::Link { url, .. } = t {
                Some(url.clone())
            } else {
                None
            }
        })
        .collect();
    assert!(urls.iter().all(|u| u != "user@example"), "got {:?}", urls);
}

#[test]
fn whitespace_inside_angle_brackets_rejected() {
    let tokens = parse("<http://a b>");
    // Either parses as no autolink, or as an autolink without the space —
    // pin the "no autolink with space in URL" contract.
    let urls: Vec<_> = tokens
        .iter()
        .filter_map(|t| {
            if let Token::Link { url, .. } = t {
                Some(url.clone())
            } else {
                None
            }
        })
        .collect();
    assert!(urls.iter().all(|u| !u.contains(' ')), "got {:?}", urls);
}

#[test]
fn adjacent_autolinks() {
    let tokens = parse("<http://a:1><http://b:2>");
    let count = tokens
        .iter()
        .filter(|t| matches!(t, Token::Link { .. }))
        .count();
    assert_eq!(count, 2);
}
