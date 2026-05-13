//! Direct tests for `parse_table` and `is_table_start` — covering column
//! count, alignment delimiters, cell content, escaped pipes, and the
//! mismatch / fallback paths.

use markdown2pdf::markdown::*;

use super::common::parse;


fn first_table(tokens: &[Token]) -> (&Vec<Vec<Token>>, &Vec<genpdfi::Alignment>, &Vec<Vec<Vec<Token>>>) {
    let Some(Token::Table { headers, aligns, rows }) =
        tokens.iter().find(|t| matches!(t, Token::Table { .. }))
    else {
        panic!("expected Table, got {:?}", tokens);
    };
    (headers, aligns, rows)
}

#[test]
fn basic_table() {
    let tokens = parse("| a | b |\n| --- | --- |\n| 1 | 2 |\n");
    let (headers, _, rows) = first_table(&tokens);
    assert_eq!(headers.len(), 2);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 2);
}

#[test]
fn table_requires_outer_pipes() {
    // This lexer requires outer pipes to detect tables; without them the
    // input is paragraph text. Pin that contract.
    let tokens = parse("a | b\n--- | ---\n1 | 2\n");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Table { .. })));
}

#[test]
fn single_column_table() {
    let tokens = parse("| a |\n| --- |\n| x |\n");
    let (headers, _, rows) = first_table(&tokens);
    assert_eq!(headers.len(), 1);
    assert_eq!(rows[0].len(), 1);
    assert_eq!(Token::collect_all_text(&rows[0][0]), "x");
}

#[test]
fn alignment_left() {
    let tokens = parse("| a |\n| :--- |\n| x |\n");
    let (_, aligns, _) = first_table(&tokens);
    assert!(matches!(aligns[0], genpdfi::Alignment::Left));
}

#[test]
fn alignment_right() {
    let tokens = parse("| a |\n| ---: |\n| x |\n");
    let (_, aligns, _) = first_table(&tokens);
    assert!(matches!(aligns[0], genpdfi::Alignment::Right));
}

#[test]
fn alignment_center() {
    let tokens = parse("| a |\n| :---: |\n| x |\n");
    let (_, aligns, _) = first_table(&tokens);
    assert!(matches!(aligns[0], genpdfi::Alignment::Center));
}

#[test]
fn alignment_default() {
    // No `:` on either side — default alignment.
    let tokens = parse("| a |\n| --- |\n| x |\n");
    let (_, aligns, _) = first_table(&tokens);
    let _ = aligns[0]; // present, no assertion on the exact default
    assert_eq!(aligns.len(), 1);
}

#[test]
fn mixed_alignments_per_column() {
    let tokens = parse("| a | b | c |\n| :--- | :---: | ---: |\n| 1 | 2 | 3 |\n");
    let (_, aligns, _) = first_table(&tokens);
    assert_eq!(aligns.len(), 3);
}

#[test]
fn row_with_fewer_cells_pads() {
    // Header has 3, row has 2 — implementation pads or truncates; either
    // way the row should not panic and the resulting table is well-formed.
    let tokens = parse("| a | b | c |\n| --- | --- | --- |\n| 1 | 2 |\n");
    let (headers, _, rows) = first_table(&tokens);
    assert_eq!(headers.len(), 3);
    assert!(!rows.is_empty());
}

#[test]
fn row_with_more_cells_keeps_all() {
    // Pin current behavior: extra cells are NOT truncated to header width;
    // the row carries all cells the user typed.
    let tokens = parse("| a | b |\n| --- | --- |\n| 1 | 2 | 3 |\n");
    let (headers, _, rows) = first_table(&tokens);
    assert_eq!(headers.len(), 2);
    assert_eq!(rows[0].len(), 3);
}

#[test]
fn escaped_pipe_currently_still_splits_known_gap() {
    // Pin current behavior: `\|` does NOT yet prevent column splitting in
    // this lexer (a known GFM gap). When the gap is closed, flip this
    // assertion to `rows[0].len() == 2`.
    let tokens = parse(r"| a | b |
| --- | --- |
| x \| y | z |
");
    let (_, _, rows) = first_table(&tokens);
    assert_eq!(rows[0].len(), 3, "got {:?}", rows[0]);
}

#[test]
fn cell_with_emphasis() {
    let tokens = parse("| a |\n| --- |\n| *x* |\n");
    let (_, _, rows) = first_table(&tokens);
    assert!(rows[0][0].iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn cell_with_inline_code() {
    let tokens = parse("| a |\n| --- |\n| `x` |\n");
    let (_, _, rows) = first_table(&tokens);
    assert!(rows[0][0].iter().any(|t| matches!(t, Token::Code { block: false, .. })));
}

#[test]
fn cell_with_link() {
    let tokens = parse("| a |\n| --- |\n| [t](u) |\n");
    let (_, _, rows) = first_table(&tokens);
    assert!(rows[0][0].iter().any(|t| matches!(t, Token::Link { .. })));
}

#[test]
fn cell_with_strikethrough() {
    let tokens = parse("| a |\n| --- |\n| ~~x~~ |\n");
    let (_, _, rows) = first_table(&tokens);
    assert!(rows[0][0].iter().any(|t| matches!(t, Token::Strikethrough(_))));
}

#[test]
fn missing_alignment_row_is_not_a_table() {
    let tokens = parse("| a | b |\n| 1 | 2 |\n");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Table { .. })));
}

#[test]
fn two_back_to_back_tables() {
    let tokens = parse(
        "| a |\n| --- |\n| 1 |\n\n| b |\n| --- |\n| 2 |\n"
    );
    let tables: Vec<_> = tokens.iter().filter(|t| matches!(t, Token::Table { .. })).collect();
    assert_eq!(tables.len(), 2, "expected two tables, got {:?}", tokens);
}

#[test]
fn empty_cell() {
    let tokens = parse("| a | b |\n| --- | --- |\n|  | x |\n");
    let (_, _, rows) = first_table(&tokens);
    assert_eq!(Token::collect_all_text(&rows[0][0]), "");
}
