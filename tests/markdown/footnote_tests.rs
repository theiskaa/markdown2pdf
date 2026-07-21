//! Direct lexer tests for GFM footnotes. Covers inline references,
//! block definitions, and the edge cases where the `[^...]` syntax
//! could collide with regular link parsing.

use super::common::parse;
use markdown2pdf::markdown::Token;

fn refs_of(tokens: &[Token]) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(t: &Token, out: &mut Vec<String>) {
        match t {
            Token::FootnoteReference(label) => out.push(label.clone()),
            Token::Heading(inner, _)
            | Token::Emphasis { content: inner, .. }
            | Token::StrongEmphasis(inner)
            | Token::Strikethrough(inner)
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

fn defs_of(tokens: &[Token]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for t in tokens {
        if let Token::FootnoteDefinition { label, content } = t {
            out.push((label.clone(), Token::collect_all_text(content)));
        }
    }
    out
}

#[test]
fn reference_with_numeric_label() {
    let tokens = parse("Text[^1].");
    assert_eq!(refs_of(&tokens), vec!["1".to_string()]);
}

#[test]
fn reference_with_alphabetic_label() {
    let tokens = parse("Text[^note].");
    assert_eq!(refs_of(&tokens), vec!["note".to_string()]);
}

#[test]
fn reference_with_alphanumeric_label() {
    let tokens = parse("Text[^a1b2].");
    assert_eq!(refs_of(&tokens), vec!["a1b2".to_string()]);
}

#[test]
fn reference_with_dash_in_label() {
    let tokens = parse("Text[^my-note].");
    assert_eq!(refs_of(&tokens), vec!["my-note".to_string()]);
}

#[test]
fn reference_with_underscore_in_label() {
    let tokens = parse("Text[^a_b].");
    assert_eq!(refs_of(&tokens), vec!["a_b".to_string()]);
}

#[test]
fn reference_at_start_of_line_is_parsed() {
    // Block-start condition tries definition first, but no `:`
    // follows so the parser falls back to inline reference.
    let tokens = parse("[^1] starts the line.");
    assert_eq!(refs_of(&tokens), vec!["1".to_string()]);
}

#[test]
fn multiple_references_on_one_line() {
    let tokens = parse("First[^1] then[^2] and[^3].");
    assert_eq!(
        refs_of(&tokens),
        vec!["1".to_string(), "2".to_string(), "3".to_string()]
    );
}

#[test]
fn repeated_reference_with_same_label() {
    // The lexer doesn't deduplicate — both occurrences are emitted.
    // Numbering / dedup happens at lower time.
    let tokens = parse("Body[^1] then again[^1].");
    assert_eq!(refs_of(&tokens), vec!["1".to_string(), "1".to_string()]);
}

#[test]
fn reference_inside_emphasis() {
    let tokens = parse("*emphasized text[^1] inside*");
    assert_eq!(refs_of(&tokens), vec!["1".to_string()]);
}

#[test]
fn reference_inside_strong_emphasis() {
    let tokens = parse("**bold text[^1] inside**");
    assert_eq!(refs_of(&tokens), vec!["1".to_string()]);
}

#[test]
fn empty_label_falls_back_to_text() {
    let tokens = parse("Text [^] more.");
    // `[^]` has no label chars between `^` and `]`. Should NOT
    // produce a FootnoteReference.
    assert_eq!(refs_of(&tokens), Vec::<String>::new());
}

#[test]
fn unclosed_reference_falls_back_to_text() {
    let tokens = parse("Text [^1 missing close.");
    assert_eq!(refs_of(&tokens), Vec::<String>::new());
}

#[test]
fn label_with_invalid_chars_falls_back() {
    // `!` is not a valid label character, so the parser fails and
    // the bracket falls through to link parsing (which then also
    // fails and emits literal text).
    let tokens = parse("Text [^a!b] more.");
    assert_eq!(refs_of(&tokens), Vec::<String>::new());
}

#[test]
fn link_without_caret_unaffected() {
    let tokens = parse("[just a link](https://example.com)");
    // No footnote refs.
    assert_eq!(refs_of(&tokens), Vec::<String>::new());
    // Link token still present.
    let has_link = tokens.iter().any(|t| matches!(t, Token::Link { .. }));
    assert!(has_link, "regular link parsing broke");
}

#[test]
fn reference_followed_by_punctuation() {
    let tokens = parse("Sentence ending with note[^1], comma.");
    assert_eq!(refs_of(&tokens), vec!["1".to_string()]);
}

#[test]
fn definition_with_simple_content() {
    let tokens = parse("[^1]: First definition");
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].0, "1");
    assert!(defs[0].1.contains("First definition"));
}

#[test]
fn definition_with_alphanumeric_label() {
    let tokens = parse("[^abc]: Some text");
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].0, "abc");
}

#[test]
fn definition_body_inline_emphasis_is_parsed() {
    let tokens = parse("[^1]: Definition with *emphasis* in it");
    for t in &tokens {
        if let Token::FootnoteDefinition { label, content } = t {
            assert_eq!(label, "1");
            let has_emphasis = content.iter().any(|c| matches!(c, Token::Emphasis { .. }));
            assert!(
                has_emphasis,
                "expected parsed Emphasis token in definition body, got {:?}",
                content
            );
            return;
        }
    }
    panic!("no FootnoteDefinition emitted");
}

#[test]
fn definition_only_at_line_start() {
    // A `[^1]: ...` appearing mid-paragraph is NOT a definition.
    let tokens = parse("Body text [^1]: not a def");
    assert_eq!(defs_of(&tokens), Vec::<(String, String)>::new());
}

#[test]
fn multiple_definitions_each_become_a_token() {
    let tokens = parse("[^1]: First\n[^2]: Second\n[^abc]: Third");
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 3);
    assert_eq!(defs[0].0, "1");
    assert_eq!(defs[1].0, "2");
    assert_eq!(defs[2].0, "abc");
}

#[test]
fn definition_body_link_is_parsed() {
    let tokens = parse("[^1]: See [example](https://example.com)");
    for t in &tokens {
        if let Token::FootnoteDefinition { label, content } = t {
            assert_eq!(label, "1");
            let link = content.iter().find_map(|c| {
                if let Token::Link { url, .. } = c {
                    Some(url.clone())
                } else {
                    None
                }
            });
            assert_eq!(link.as_deref(), Some("https://example.com"));
            return;
        }
    }
    panic!("no FootnoteDefinition emitted");
}

#[test]
fn reference_and_definition_in_same_document() {
    let tokens = parse("Body text[^1].\n[^1]: Note");
    assert_eq!(refs_of(&tokens), vec!["1".to_string()]);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].0, "1");
}

#[test]
fn unused_definition_still_lexed() {
    let tokens = parse("[^orphan]: Nobody references me");
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].0, "orphan");
}

#[test]
fn forward_reference_before_definition() {
    let tokens = parse("Body[^later].\n\nMore body.\n\n[^later]: definition");
    assert_eq!(refs_of(&tokens), vec!["later".to_string()]);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
}

#[test]
fn definition_with_empty_body_lexes() {
    let tokens = parse("[^1]:");
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].0, "1");
    // Empty body — content is empty or whitespace only.
    assert!(defs[0].1.trim().is_empty());
}

#[test]
fn multiline_definition_joins_indented_continuation() {
    // GFM: 4-space-indented continuation lines become part of the
    // same definition body, joined by a soft space.
    let src = "[^1]: First line.\n    Second line continues.";
    let tokens = parse(src);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert!(
        defs[0].1.contains("First line.") && defs[0].1.contains("Second line continues."),
        "multi-line body lost content: {:?}",
        defs[0].1
    );
}

#[test]
fn multiline_definition_supports_three_continuation_lines() {
    let src = "[^1]: Line one.\n    Line two.\n    Line three.\n    Line four.";
    let tokens = parse(src);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    for needle in ["Line one.", "Line two.", "Line three.", "Line four."] {
        assert!(
            defs[0].1.contains(needle),
            "multi-line body missing `{}` (got {:?})",
            needle,
            defs[0].1
        );
    }
}

#[test]
fn multiline_definition_stops_at_blank_line() {
    // The blank line terminates the body; the paragraph after stays
    // a regular paragraph, not part of the footnote.
    let src = "[^1]: Inside footnote.\n\nNot in footnote.";
    let tokens = parse(src);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert!(defs[0].1.contains("Inside footnote."));
    assert!(
        !defs[0].1.contains("Not in footnote"),
        "blank line should have ended the body: {:?}",
        defs[0].1
    );
}

#[test]
fn multiline_definition_stops_at_unindented_line() {
    let src = "[^1]: First line.\nNot indented.";
    let tokens = parse(src);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert!(defs[0].1.contains("First line."));
    assert!(
        !defs[0].1.contains("Not indented"),
        "non-indented line should have ended the body: {:?}",
        defs[0].1
    );
}

#[test]
fn multiline_definition_indent_can_be_tab() {
    // A leading tab counts as ≥4 columns of indentation per GFM.
    let src = "[^1]: First line.\n\tSecond line via tab.";
    let tokens = parse(src);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert!(
        defs[0].1.contains("First line.") && defs[0].1.contains("Second line via tab."),
        "tab continuation not joined: {:?}",
        defs[0].1
    );
}

#[test]
fn multiline_definition_indent_requires_four_spaces() {
    // Only 3 spaces of indent: NOT a continuation. The body is the
    // first line only.
    let src = "[^1]: First line.\n   Three-space indent.";
    let tokens = parse(src);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert!(defs[0].1.contains("First line."));
    assert!(
        !defs[0].1.contains("Three-space indent"),
        "3-space indent should NOT be a continuation: {:?}",
        defs[0].1
    );
}

#[test]
fn multiline_definition_continuation_runs_inline_lexer() {
    // Inline markdown inside continuation lines should be parsed
    // (emphasis, links, code) just like the first line.
    let src = "[^1]: First.\n    Second with *emphasis* and `code`.";
    let tokens = parse(src);
    for t in &tokens {
        if let Token::FootnoteDefinition { label, content } = t {
            assert_eq!(label, "1");
            let has_emphasis = content.iter().any(|c| matches!(c, Token::Emphasis { .. }));
            let has_code = content
                .iter()
                .any(|c| matches!(c, Token::Code { block: false, .. }));
            assert!(has_emphasis, "no emphasis from continuation: {:?}", content);
            assert!(has_code, "no inline code from continuation: {:?}", content);
            return;
        }
    }
    panic!("no FootnoteDefinition emitted");
}

#[test]
fn multiline_definition_followed_by_another_definition() {
    // Multiple consecutive multi-line definitions all parse cleanly.
    let src = "[^1]: First definition.\n    Second line of first.\n[^2]: Second definition.\n    Second line of second.";
    let tokens = parse(src);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 2);
    assert_eq!(defs[0].0, "1");
    assert_eq!(defs[1].0, "2");
    assert!(defs[0].1.contains("Second line of first."));
    assert!(defs[1].1.contains("Second line of second."));
    assert!(
        !defs[0].1.contains("Second line of second"),
        "definitions leaked content between each other: {:?}",
        defs[0].1
    );
}

#[test]
fn singleline_definition_still_works() {
    // Sanity: pre-existing single-line behavior is unchanged.
    let src = "[^1]: Just one line.";
    let tokens = parse(src);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].1, "Just one line.");
}

#[test]
fn multiline_definition_does_not_consume_following_unindented_paragraph() {
    // Regression: paragraphs after a footnote definition should
    // remain regular paragraphs, not get absorbed.
    let src = "[^1]: A footnote.\n    Continued.\n\nNew paragraph after blank line.";
    let tokens = parse(src);
    let defs = defs_of(&tokens);
    assert_eq!(defs.len(), 1);
    assert!(defs[0].1.contains("A footnote."));
    assert!(defs[0].1.contains("Continued."));
    let all_text = Token::collect_all_text(&tokens);
    assert!(
        all_text.contains("New paragraph after blank line."),
        "following paragraph was swallowed: {:?}",
        all_text
    );
}
