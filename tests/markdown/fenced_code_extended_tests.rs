//! Extended tests for fenced code blocks (`parse_code`, `parse_tilde_fence`,
//! `count_backticks`). Existing `fenced_code_info_string_tests` covers
//! info-string parsing; this module focuses on fence-kind matching, length
//! rules, indent handling, and context placement.

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_code_block(input: &str) -> (String, String) {
    let tokens = parse(input);
    let Some(Token::Code {
        language,
        content,
        block: true,
    }) = tokens
        .iter()
        .find(|t| matches!(t, Token::Code { block: true, .. }))
    else {
        panic!("expected fenced/indented Code block, got {:?}", tokens);
    };
    (language.clone(), content.clone())
}

#[test]
fn backtick_fence_basic() {
    let (lang, body) = first_code_block("```\nbody\n```");
    assert_eq!(lang, "");
    assert_eq!(body, "body");
}

#[test]
fn tilde_fence_basic() {
    let (lang, body) = first_code_block("~~~\nbody\n~~~");
    assert_eq!(lang, "");
    assert_eq!(body, "body");
}

#[test]
fn tilde_does_not_close_backtick() {
    // The closer must match the opener kind. `~~~` after ``` ``` keeps the
    // backtick fence open and produces a code block with `~~~` inside.
    let (_, body) = first_code_block("```\nbody\n~~~\nmore\n```");
    assert!(body.contains("~~~"));
    assert!(body.contains("more"));
}

#[test]
fn backtick_does_not_close_tilde() {
    let (_, body) = first_code_block("~~~\nbody\n```\nmore\n~~~");
    assert!(body.contains("```"));
    assert!(body.contains("more"));
}

#[test]
fn closing_fence_length_must_be_at_least_opening() {
    // Opener is 4 backticks; 3-backtick fence in body doesn't close.
    let (_, body) = first_code_block("````\nbody ``` still in\n````");
    assert!(body.contains("```"));
}

#[test]
fn tilde_fence_info_string_may_contain_backticks() {
    let (lang, _) = first_code_block("~~~rust `template`\nbody\n~~~");
    assert_eq!(lang, "rust");
}

#[test]
fn one_space_indent_fence_body_strips_one_col() {
    let (_, body) = first_code_block(" ```\n body\n ```");
    assert_eq!(body, "body");
}

#[test]
fn three_space_indent_fence_body_strips_three_cols() {
    let (_, body) = first_code_block("   ```\n   body\n   ```");
    assert_eq!(body, "body");
}

#[test]
fn four_space_indent_is_indented_code_block() {
    // 4-space indent before fence is too much — opens an indented code block
    // instead. The triple backticks become literal text in that block.
    let (_, body) = first_code_block("    ```\n    body\n");
    assert!(body.contains("```"), "got {:?}", body);
}

#[test]
fn unterminated_fence_runs_to_eof() {
    let (_, body) = first_code_block("```\nbody line\n");
    assert!(body.contains("body line"));
}

#[test]
fn info_string_only_first_word_becomes_language() {
    let (lang, _) = first_code_block("```rust hint extra\nbody\n```");
    assert_eq!(lang, "rust");
}

#[test]
fn fenced_code_inside_blockquote() {
    let tokens = parse("> ```\n> body\n> ```\n");
    let Some(Token::BlockQuote(body)) = tokens.first() else {
        panic!("expected BlockQuote, got {:?}", tokens);
    };
    assert!(
        body.iter()
            .any(|t| matches!(t, Token::Code { block: true, .. }))
    );
}

#[test]
fn empty_info_string_is_empty_language() {
    let (lang, _) = first_code_block("```\n\n```");
    assert_eq!(lang, "");
}
