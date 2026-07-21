//! Direct lexer tests for Pandoc-style inline footnotes
//! (`text^[note body]`). Covers the happy path, balanced-bracket and
//! escape handling inside the body, label uniqueness across nested
//! sub-lexer content, and the graceful degradation cases where `^[`
//! is not a well-formed inline footnote.

use super::common::parse;
use markdown2pdf::markdown::Token;

/// Collect every `InlineFootnote` in document order as
/// `(label, body-text)` pairs, descending into the usual container
/// tokens so footnotes inside emphasis / lists / etc. are found.
fn inline_notes(tokens: &[Token]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    fn walk(t: &Token, out: &mut Vec<(String, String)>) {
        match t {
            Token::InlineFootnote { label, content } => {
                out.push((label.clone(), Token::collect_all_text(content)));
                for c in content {
                    walk(c, out);
                }
            }
            Token::Heading(inner, _)
            | Token::Emphasis { content: inner, .. }
            | Token::StrongEmphasis(inner)
            | Token::Strikethrough(inner)
            | Token::Highlight(inner)
            | Token::BlockQuote(inner)
            | Token::ListItem { content: inner, .. }
            | Token::Link { content: inner, .. }
            | Token::Image { alt: inner, .. }
            | Token::FootnoteDefinition { content: inner, .. } => {
                for c in inner {
                    walk(c, out);
                }
            }
            _ => {}
        }
    }
    for t in tokens {
        walk(t, &mut out);
    }
    out
}

#[test]
fn simple_inline_footnote_emits_token() {
    let tokens = parse("Here is a statement^[and here is the note].");
    let notes = inline_notes(&tokens);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].1, "and here is the note");
    // Surrounding text is preserved on both sides of the marker.
    let all = Token::collect_all_text(&tokens);
    assert!(
        all.contains("Here is a statement"),
        "lead text lost: {all:?}"
    );
    assert!(all.contains('.'), "trailing punctuation lost: {all:?}");
}

#[test]
fn body_parses_inline_markdown() {
    let tokens = parse("x^[see *this* and `that`]");
    let mut found = false;
    fn check(t: &Token, found: &mut bool) {
        if let Token::InlineFootnote { content, .. } = t {
            let has_em = content.iter().any(|c| matches!(c, Token::Emphasis { .. }));
            let has_code = content
                .iter()
                .any(|c| matches!(c, Token::Code { block: false, .. }));
            assert!(has_em, "emphasis not parsed in body: {content:?}");
            assert!(has_code, "code span not parsed in body: {content:?}");
            *found = true;
        }
    }
    for t in &tokens {
        check(t, &mut found);
    }
    assert!(found, "no InlineFootnote emitted");
}

#[test]
fn nested_brackets_in_body_are_balanced() {
    // The body runs to the `]` that balances the opening `[`, so the
    // inner `[1]` pair stays inside it.
    let tokens = parse("ref^[see [1] in the list] done");
    let notes = inline_notes(&tokens);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].1, "see [1] in the list");
    let all = Token::collect_all_text(&tokens);
    assert!(
        all.contains("done"),
        "text after the note was eaten: {all:?}"
    );
}

#[test]
fn escaped_close_bracket_does_not_terminate_body() {
    let tokens = parse(r"x^[a \] still inside] y");
    let notes = inline_notes(&tokens);
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].1.contains(']'),
        "escaped bracket missing from body: {:?}",
        notes[0].1
    );
}

#[test]
fn two_inline_footnotes_get_distinct_labels() {
    let tokens = parse("a^[one] and b^[two]");
    let notes = inline_notes(&tokens);
    assert_eq!(notes.len(), 2);
    assert_ne!(notes[0].0, notes[1].0, "labels collided: {notes:?}");
    assert_eq!(notes[0].1, "one");
    assert_eq!(notes[1].1, "two");
}

#[test]
fn labels_stay_unique_across_list_items() {
    // List items are parsed by sub-lexers; the seq counter is shared
    // so labels must not collide between them.
    let tokens = parse("- a^[x]\n- b^[y]\n");
    let notes = inline_notes(&tokens);
    assert_eq!(notes.len(), 2);
    assert_ne!(notes[0].0, notes[1].0, "labels collided across list items");
}

#[test]
fn inline_footnote_inside_emphasis() {
    let tokens = parse("*emphasized ^[a note] text*");
    let notes = inline_notes(&tokens);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].1, "a note");
}

#[test]
fn unbalanced_degrades_to_literal_text() {
    let tokens = parse("x^[unterminated note");
    assert!(inline_notes(&tokens).is_empty());
    let all = Token::collect_all_text(&tokens);
    assert!(
        all.contains("^[unterminated note"),
        "unterminated body should stay literal: {all:?}"
    );
}

#[test]
fn empty_body_degrades_to_literal_text() {
    let tokens = parse("x^[] y");
    assert!(inline_notes(&tokens).is_empty());
    let all = Token::collect_all_text(&tokens);
    assert!(
        all.contains("^[]"),
        "empty body should stay literal: {all:?}"
    );
}

#[test]
fn bare_caret_is_literal() {
    for src in ["2^3 is eight", "a ^ b", "x^y"] {
        let tokens = parse(src);
        assert!(
            inline_notes(&tokens).is_empty(),
            "false positive for {src:?}"
        );
        assert_eq!(
            Token::collect_all_text(&tokens),
            src,
            "bare caret text changed"
        );
    }
}

#[test]
fn regular_footnote_reference_unaffected() {
    let tokens = parse("Text[^1].\n\n[^1]: def");
    // No inline footnotes here; the `[^1]` path is untouched.
    assert!(inline_notes(&tokens).is_empty());
    let has_ref = tokens.iter().any(|t| {
        fn any_ref(t: &Token) -> bool {
            match t {
                Token::FootnoteReference(_) => true,
                Token::Heading(i, _) | Token::ListItem { content: i, .. } => i.iter().any(any_ref),
                _ => false,
            }
        }
        any_ref(t)
    });
    assert!(has_ref, "regular footnote reference parsing broke");
}
