use markdown2pdf::markdown::*;

use super::common::parse;

fn fence(input: &str) -> (String, String) {
    let tokens = parse(input);
    for t in &tokens {
        if let Token::Code {
            language: lang,
            content: body,
            ..
        } = t
        {
            return (lang.clone(), body.clone());
        }
    }
    panic!(
        "expected Code token, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn backtick_fence_simple_language() {
    let (lang, _) = fence("```rust\nfn x() {}\n```");
    assert_eq!(lang, "rust");
}

#[test]
fn backtick_fence_language_with_trailing_metadata() {
    let (lang, _) = fence("```rust title=\"example\" linenos\nfn x() {}\n```");
    assert_eq!(lang, "rust", "info-string metadata must not be in language");
}

#[test]
fn backtick_fence_empty_info_string() {
    let (lang, _) = fence("```\nplain\n```");
    assert_eq!(lang, "");
}

#[test]
fn backtick_fence_whitespace_only_info_string() {
    let (lang, _) = fence("```   \nplain\n```");
    assert_eq!(lang, "");
}

#[test]
fn backtick_fence_language_trimmed() {
    let (lang, _) = fence("```   rust   \ncode\n```");
    assert_eq!(lang, "rust");
}

#[test]
fn tilde_fence_simple_language() {
    let (lang, _) = fence("~~~python\nprint('hi')\n~~~");
    assert_eq!(lang, "python");
}

#[test]
fn tilde_fence_language_with_metadata() {
    let (lang, _) = fence("~~~ts strict=true\ntype A = number;\n~~~");
    assert_eq!(lang, "ts");
}

#[test]
fn tilde_fence_allows_backticks_in_info_string() {
    let (lang, _) = fence("~~~`backticks` allowed here\ncontent\n~~~");
    assert_eq!(lang, "`backticks`");
}

#[test]
fn tilde_fence_empty_info_string() {
    let (lang, _) = fence("~~~\nplain\n~~~");
    assert_eq!(lang, "");
}

#[test]
fn backtick_fence_with_backticks_in_info_string_is_inline_span() {
    // a backtick fence's info string may not contain any
    // backticks — so this opens an inline span instead. Discriminator:
    // a real fence would put `body` in content with the info string
    // dropped; the inline-span fallback includes the info-string text
    // (`bad` literal) inside the span body.
    let tokens = parse("``` `bad` info\nbody\n```");
    let inline_span_with_info_text = tokens
        .iter()
        .any(|t| matches!(t, Token::Code { content: body, .. } if body.contains("bad")));
    assert!(
        inline_span_with_info_text,
        "expected inline span carrying info-string text, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn fence_body_unchanged_by_info_string_split() {
    let (lang, body) = fence("```rust meta1 meta2\nlet x = 1;\nlet y = 2;\n```");
    assert_eq!(lang, "rust");
    assert!(body.contains("let x = 1;"));
    assert!(body.contains("let y = 2;"));
}
