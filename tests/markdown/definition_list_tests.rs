//! Lexer coverage for PHP Markdown Extra-style definition lists.

use markdown2pdf::markdown::{DefinitionListEntry, Token};

use super::common::parse;

fn first_definition_list(tokens: &[Token]) -> Option<&Vec<DefinitionListEntry>> {
    tokens.iter().find_map(|t| match t {
        Token::DefinitionList { entries } => Some(entries),
        _ => None,
    })
}

fn term_text(entry: &DefinitionListEntry) -> String {
    Token::collect_all_text(&entry.terms[0])
}

fn definition_text(entry: &DefinitionListEntry, idx: usize) -> String {
    Token::collect_all_text(&entry.definitions[idx])
}

#[test]
fn single_term_with_single_definition() {
    let toks = parse("Apple\n: A red fruit.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 1);
    assert_eq!(term_text(&entries[0]), "Apple");
    assert_eq!(entries[0].definitions.len(), 1);
    assert_eq!(definition_text(&entries[0], 0), "A red fruit.");
}

#[test]
fn term_with_multiple_definitions() {
    let toks = parse("Color\n: Red, green, blue.\n: A wavelength.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].definitions.len(), 2);
    assert_eq!(definition_text(&entries[0], 0), "Red, green, blue.");
    assert_eq!(definition_text(&entries[0], 1), "A wavelength.");
}

#[test]
fn two_consecutive_entries() {
    let toks = parse("Apple\n: A red fruit.\nBanana\n: A yellow fruit.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 2);
    assert_eq!(term_text(&entries[0]), "Apple");
    assert_eq!(term_text(&entries[1]), "Banana");
}

#[test]
fn entries_separated_by_one_blank_line() {
    let toks = parse("Apple\n: Red.\n\nBanana\n: Yellow.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 2);
    assert_eq!(term_text(&entries[1]), "Banana");
}

#[test]
fn two_blank_lines_separate_into_distinct_lists() {
    let toks = parse("Apple\n: Red.\n\n\nBanana\n: Yellow.\n");
    let lists: Vec<&Vec<DefinitionListEntry>> = toks
        .iter()
        .filter_map(|t| match t {
            Token::DefinitionList { entries } => Some(entries),
            _ => None,
        })
        .collect();
    assert_eq!(lists.len(), 2);
    assert_eq!(lists[0].len(), 1);
    assert_eq!(lists[1].len(), 1);
}

#[test]
fn definition_with_indented_colon() {
    let toks = parse("Apple\n   : Red.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 1);
    assert_eq!(definition_text(&entries[0], 0), "Red.");
}

#[test]
fn four_space_indented_colon_is_not_definition_list() {
    // 4+ leading spaces is an indented code block, not a definition marker.
    let toks = parse("Apple\n    : Red.\n");
    assert!(first_definition_list(&toks).is_none());
}

#[test]
fn colon_without_space_is_not_definition_marker() {
    let toks = parse("Apple\n:NoSpace\n");
    assert!(first_definition_list(&toks).is_none());
}

#[test]
fn empty_definition_body_is_allowed() {
    let toks = parse("Apple\n: \nNext line.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 1);
    assert_eq!(definition_text(&entries[0], 0), "");
}

#[test]
fn term_with_punctuation() {
    let toks = parse("Question?\n: Answer here.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(term_text(&entries[0]), "Question?");
}

#[test]
fn term_with_spaces() {
    let toks = parse("Hot cocoa\n: A warm winter drink.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(term_text(&entries[0]), "Hot cocoa");
}

#[test]
fn heading_term_is_not_a_definition_list() {
    // `#` opens an ATX heading; the next line's `:` must NOT pull the
    // heading into a definition list.
    let toks = parse("# Heading\n: Should be paragraph.\n");
    assert!(first_definition_list(&toks).is_none());
    assert!(toks.iter().any(|t| matches!(t, Token::Heading(_, 1))));
}

#[test]
fn list_marker_term_is_not_a_definition_list() {
    let toks = parse("- Item\n: Not a definition.\n");
    assert!(first_definition_list(&toks).is_none());
}

#[test]
fn blockquote_term_is_not_a_definition_list() {
    let toks = parse("> Quote\n: Not a definition.\n");
    assert!(first_definition_list(&toks).is_none());
}

#[test]
fn fence_term_is_not_a_definition_list() {
    let toks = parse("```\n: Inside fence.\n```\n");
    assert!(first_definition_list(&toks).is_none());
}

#[test]
fn ordered_marker_term_is_not_a_definition_list() {
    let toks = parse("1. Item\n: Not a definition.\n");
    assert!(first_definition_list(&toks).is_none());
}

#[test]
fn standalone_colon_paragraph_unaffected() {
    let toks = parse(": Just text.\n");
    assert!(first_definition_list(&toks).is_none());
}

#[test]
fn definition_list_after_paragraph_separated_by_blank_line() {
    // After a blank line, a fresh term + : pattern should start a list.
    let toks = parse("Intro paragraph.\n\nTerm\n: Definition.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 1);
    assert_eq!(term_text(&entries[0]), "Term");
}

#[test]
fn three_entries_in_one_list() {
    let toks = parse("Apple\n: Red.\nBanana\n: Yellow.\nCherry\n: Red.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 3);
}

#[test]
fn unicode_term_and_definition() {
    let toks = parse("Café\n: A warm drink.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(term_text(&entries[0]), "Café");
}

#[test]
fn collect_all_text_includes_definition_list_content() {
    let toks = parse("Term\n: A definition.\n");
    let collected = Token::collect_all_text(&toks);
    assert!(collected.contains("Term"));
    assert!(collected.contains("A definition."));
}

#[test]
fn multi_term_shares_definitions() {
    let toks = parse("Alpha\nBeta\n: Shared definition.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].terms.len(), 2);
    assert_eq!(Token::collect_all_text(&entries[0].terms[0]), "Alpha");
    assert_eq!(Token::collect_all_text(&entries[0].terms[1]), "Beta");
    assert_eq!(entries[0].definitions.len(), 1);
}

#[test]
fn second_colon_block_after_blank_line_is_another_definition() {
    let toks = parse("Epsilon\n: First.\n\n: Second.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].definitions.len(), 2);
    assert_eq!(definition_text(&entries[0], 0), "First.");
    assert_eq!(definition_text(&entries[0], 1), "Second.");
}

#[test]
fn definition_body_with_indented_code_block() {
    let toks = parse("Term\n:   First paragraph.\n\n    ```rust\n    let x = 42;\n    ```\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    let def = &entries[0].definitions[0];
    assert!(
        def.iter()
            .any(|t| matches!(t, Token::Code { block: true, .. })),
        "expected a fenced Code block inside definition body, got {def:?}"
    );
}

#[test]
fn definition_body_with_multiple_paragraphs() {
    let toks =
        parse("Term\n:   First paragraph.\n\n    Second paragraph still part of definition.\n");
    let entries = first_definition_list(&toks).expect("expected DefinitionList");
    let def = &entries[0].definitions[0];
    let text = Token::collect_all_text(def);
    assert!(text.contains("First paragraph"));
    assert!(text.contains("Second paragraph still part of definition"));
}
