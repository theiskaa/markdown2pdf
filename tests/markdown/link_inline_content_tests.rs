use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn link_text_parses_emphasis() {
    let tokens = parse("[*emph* text](u)");
    let Token::Link { content, .. } = &tokens[0] else {
        panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
    };
    assert!(
        content.iter().any(|t| matches!(t, Token::Emphasis { .. })),
        "expected Emphasis inside link text, got {}",
        Token::slice_to_compact(content)
    );
}

#[test]
fn link_text_parses_strong_emphasis() {
    // `**bold**` produces Emphasis with level 2 in this lexer (not a
    // separate StrongEmphasis token).
    let tokens = parse("[**bold** link](u)");
    let Token::Link { content, .. } = &tokens[0] else {
        panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
    };
    assert!(
        content
            .iter()
            .any(|t| matches!(t, Token::Emphasis { level: 2, .. })),
        "expected Emphasis level=2, got {}",
        Token::slice_to_compact(content)
    );
}

#[test]
fn link_text_parses_code_span() {
    let tokens = parse("[`code` snippet](u)");
    let Token::Link { content, .. } = &tokens[0] else {
        panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
    };
    assert!(
        content
            .iter()
            .any(|t| matches!(t, Token::Code { content: body, .. } if body == "code")),
        "expected Code span, got {}",
        Token::slice_to_compact(content)
    );
}

#[test]
fn link_text_decodes_entities() {
    let tokens = parse("[a &amp; b](u)");
    let Token::Link { content, .. } = &tokens[0] else {
        panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
    };
    let text = Token::collect_all_text(content);
    assert_eq!(text, "a & b");
}

#[test]
fn link_text_honors_backslash_escape() {
    let tokens = parse(r"[a\*not emph\*](u)");
    let Token::Link { content, .. } = &tokens[0] else {
        panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
    };
    // No Emphasis should have been produced — escapes blocked it.
    assert!(
        !content.iter().any(|t| matches!(t, Token::Emphasis { .. })),
        "escape should have blocked emphasis, got {}",
        Token::slice_to_compact(content)
    );
    let text = Token::collect_all_text(content);
    assert_eq!(text, "a*not emph*");
}

#[test]
fn image_alt_parses_inline_formatting() {
    let tokens = parse("![*alt* text](pic.png)");
    let Token::Image { alt, .. } = &tokens[0] else {
        panic!("expected Image, got {}", Token::slice_to_compact(&tokens));
    };
    assert!(
        alt.iter().any(|t| matches!(t, Token::Emphasis { .. })),
        "expected Emphasis in alt, got {}",
        Token::slice_to_compact(alt)
    );
}

#[test]
fn link_title_escape_double_quote() {
    // `\"` inside a double-quoted title produces a literal `"`.
    let tokens = parse(r#"[t](u "a\"b")"#);
    let Token::Link { title, .. } = &tokens[0] else {
        panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
    };
    assert_eq!(title.as_deref(), Some("a\"b"));
}

#[test]
fn link_title_entity_decoded() {
    let tokens = parse(r#"[t](u "a &amp; b")"#);
    let Token::Link { title, .. } = &tokens[0] else {
        panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
    };
    assert_eq!(title.as_deref(), Some("a & b"));
}

#[test]
fn link_title_with_paren_delimiter_escaped_close() {
    let tokens = parse(r"[t](u (in\) title))");
    let Token::Link { title, .. } = &tokens[0] else {
        panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
    };
    assert_eq!(title.as_deref(), Some("in) title"));
}

#[test]
fn autolink_keeps_url_as_link_text() {
    let tokens = parse("<https://example.com>");
    let Token::Link {
        content,
        url,
        title,
    } = &tokens[0]
    else {
        panic!(
            "expected autolink, got {}",
            Token::slice_to_compact(&tokens)
        );
    };
    assert_eq!(Token::collect_all_text(content), "https://example.com");
    assert_eq!(url, "https://example.com");
    assert!(title.is_none());
}
