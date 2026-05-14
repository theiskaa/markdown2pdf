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
    assert_eq!(
        refs_of(&tokens),
        vec!["1".to_string(), "1".to_string()]
    );
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
            let has_emphasis = content
                .iter()
                .any(|c| matches!(c, Token::Emphasis { .. }));
            assert!(has_emphasis, "expected parsed Emphasis token in definition body, got {:?}", content);
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
