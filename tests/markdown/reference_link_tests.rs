use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn full_reference_link() {
    let input = "[CommonMark][cm]\n\n[cm]: https://commonmark.org";
    let tokens = parse(input);
    assert!(
        tokens.iter().any(|t| matches!(
            t,
            Token::Link { content, url, .. }
            if Token::collect_all_text(content) == "CommonMark"
                && url == "https://commonmark.org"
        )),
        "got {:?}",
        tokens
    );
}

#[test]
fn collapsed_reference_link() {
    let input = "[CommonMark][]\n\n[CommonMark]: https://commonmark.org";
    let tokens = parse(input);
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Link { url, .. } if url == "https://commonmark.org")),
        "got {:?}",
        tokens
    );
}

#[test]
fn shortcut_reference_link() {
    let input = "[CommonMark]\n\n[CommonMark]: https://commonmark.org";
    let tokens = parse(input);
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Link { url, .. } if url == "https://commonmark.org")),
        "got {:?}",
        tokens
    );
}

#[test]
fn label_matching_is_case_insensitive() {
    let input = "[CommonMark][CM]\n\n[cm]: https://commonmark.org";
    let tokens = parse(input);
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Link { url, .. } if url == "https://commonmark.org")),
        "got {:?}",
        tokens
    );
}

#[test]
fn definition_line_is_not_emitted_as_text() {
    let input = "para\n\n[cm]: https://commonmark.org";
    let tokens = parse(input);
    // No token should contain the literal text "https://commonmark.org"
    // outside of a Link, since the definition line is consumed.
    let stray = tokens
        .iter()
        .any(|t| matches!(t, Token::Text(s) if s.contains("https://commonmark.org")));
    assert!(!stray, "definition line bled into output: {:?}", tokens);
}

#[test]
fn unresolved_shortcut_falls_back_to_text() {
    // `[Word]` with no matching definition should NOT become a Link
    // (today it does — empty URL — which is the bug).
    let tokens = parse("Just [Word] in text.");
    let has_link = tokens.iter().any(|t| matches!(t, Token::Link { .. }));
    assert!(
        !has_link,
        "unresolved shortcut must NOT become a link, got {:?}",
        tokens
    );
}

#[test]
fn reference_image() {
    let input = "![alt][img]\n\n[img]: pic.png";
    let tokens = parse(input);
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Image { url, .. } if url == "pic.png")),
        "got {:?}",
        tokens
    );
}

#[test]
fn definition_with_title_is_parsed_url_clean() {
    let input = "[a][r]\n\n[r]: https://example.com \"Example\"";
    let tokens = parse(input);
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Link { url, .. } if url == "https://example.com")),
        "URL should be clean (no title baked in), got {:?}",
        tokens
    );
}

#[test]
fn inline_link_still_takes_priority_over_reference() {
    // [text](url) is inline — must NOT be confused with a reference.
    let tokens = parse("[text](https://example.com)\n\n[text]: should-not-apply");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Link { url, .. } if url == "https://example.com"))
    );
}

#[test]
fn whitespace_in_label_normalized() {
    let input = "[Multi  Word  Label][m]\n\n[M  Word  Label]: https://example.com";
    let tokens = parse(input);
    let _ = tokens;
}

#[test]
fn space_after_reference_link_preserved() {
    // Text following a [t][r] reference must keep its leading space —
    // `]` should be treated like `)` by
    // is_after_special_token so skip_whitespace doesn't swallow it.
    let input = "See [the spec][cm] for details.\n\n[cm]: https://x";
    let tokens = parse(input);
    let body = Token::collect_all_text(&tokens);
    assert!(
        body.contains(" for details"),
        "expected leading space before 'for', got {:?}",
        body
    );
}

#[test]
fn space_after_shortcut_link_preserved() {
    let input = "A bare [Rust] is also a link.\n\n[Rust]: https://rust-lang.org";
    let tokens = parse(input);
    let body = Token::collect_all_text(&tokens);
    assert!(
        body.contains(" is also"),
        "expected leading space before 'is', got {:?}",
        body
    );
}

#[test]
fn space_after_collapsed_reference_preserved() {
    let input = "The [Wikipedia][] entry.\n\n[Wikipedia]: https://x";
    let tokens = parse(input);
    let body = Token::collect_all_text(&tokens);
    assert!(
        body.contains(" entry"),
        "expected leading space before 'entry', got {:?}",
        body
    );
}

#[test]
fn space_after_unresolved_shortcut_preserved() {
    let input = "Phrase [No Such Label] stays literal.";
    let tokens = parse(input);
    let body = Token::collect_all_text(&tokens);
    assert!(
        body.contains(" stays"),
        "expected leading space before 'stays', got {:?}",
        body
    );
}

#[test]
fn space_after_autolink_preserved() {
    let tokens = parse("see <https://example.com> for more");
    let body = Token::collect_all_text(&tokens);
    assert!(
        body.contains(" for "),
        "expected leading space before 'for', got {:?}",
        body
    );
}
