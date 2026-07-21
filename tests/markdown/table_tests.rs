//! Direct tests for `parse_table` and `is_table_start` — covering column
//! count, alignment delimiters, cell content, escaped pipes, and the
//! mismatch / fallback paths.

use markdown2pdf::markdown::*;

use super::common::parse;


/// `(headers, aligns, rows)` borrowed from a parsed `Token::Table`.
type TableParts<'a> = (
    &'a Vec<TableCell<Token>>,
    &'a Vec<markdown2pdf::markdown::TableAlignment>,
    &'a Vec<Vec<TableCell<Token>>>,
);

fn first_table(tokens: &[Token]) -> TableParts<'_> {
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
    assert_eq!(Token::collect_all_text(&rows[0][0].content), "x");
}

#[test]
fn alignment_left() {
    let tokens = parse("| a |\n| :--- |\n| x |\n");
    let (_, aligns, _) = first_table(&tokens);
    assert!(matches!(aligns[0], markdown2pdf::markdown::TableAlignment::Left));
}

#[test]
fn alignment_right() {
    let tokens = parse("| a |\n| ---: |\n| x |\n");
    let (_, aligns, _) = first_table(&tokens);
    assert!(matches!(aligns[0], markdown2pdf::markdown::TableAlignment::Right));
}

#[test]
fn alignment_center() {
    let tokens = parse("| a |\n| :---: |\n| x |\n");
    let (_, aligns, _) = first_table(&tokens);
    assert!(matches!(aligns[0], markdown2pdf::markdown::TableAlignment::Center));
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
    assert!(rows[0][0].content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
}

#[test]
fn cell_with_inline_code() {
    let tokens = parse("| a |\n| --- |\n| `x` |\n");
    let (_, _, rows) = first_table(&tokens);
    assert!(rows[0][0].content.iter().any(|t| matches!(t, Token::Code { block: false, .. })));
}

#[test]
fn cell_with_link() {
    let tokens = parse("| a |\n| --- |\n| [t](u) |\n");
    let (_, _, rows) = first_table(&tokens);
    assert!(rows[0][0].content.iter().any(|t| matches!(t, Token::Link { .. })));
}

#[test]
fn cell_with_strikethrough() {
    let tokens = parse("| a |\n| --- |\n| ~~x~~ |\n");
    let (_, _, rows) = first_table(&tokens);
    assert!(rows[0][0].content.iter().any(|t| matches!(t, Token::Strikethrough(_))));
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
    assert_eq!(Token::collect_all_text(&rows[0][0].content), "");
}

#[test]
fn plain_gfm_cells_have_unit_spans() {
    let tokens = parse("| a | b | c |\n| --- | --- | --- |\n| 1 |  | 3 |\n");
    let (headers, _, rows) = first_table(&tokens);
    for cell in headers.iter().chain(rows.iter().flatten()) {
        assert_eq!(cell.colspan, 1);
        assert_eq!(cell.rowspan, 1);
        assert!(!cell.covered);
    }
}

#[test]
fn marker_cell_extends_colspan() {
    let tokens = parse("| Group | > | Regular |\n| --- | --- | --- |\n| A | B | C |\n");
    let (headers, _, _) = first_table(&tokens);
    assert_eq!(headers.len(), 3);
    assert_eq!(Token::collect_all_text(&headers[0].content), "Group");
    assert_eq!(headers[0].colspan, 2);
    assert!(headers[1].covered);
    assert_eq!(Token::collect_all_text(&headers[2].content), "Regular");
}

#[test]
fn caret_cell_extends_rowspan_from_cell_above() {
    let tokens = parse("| Key | Value |\n| --- | --- |\n| A | one |\n| ^ | two |\n");
    let (_, _, rows) = first_table(&tokens);
    assert_eq!(Token::collect_all_text(&rows[0][0].content), "A");
    assert_eq!(rows[0][0].rowspan, 2);
    assert!(rows[1][0].covered);
    assert_eq!(Token::collect_all_text(&rows[1][1].content), "two");
}

#[test]
fn rowspan_chains_across_three_rows() {
    let tokens =
        parse("| Key | Value |\n| --- | --- |\n| A | 1 |\n| ^ | 2 |\n| ^ | 3 |\n");
    let (_, _, rows) = first_table(&tokens);
    assert_eq!(Token::collect_all_text(&rows[0][0].content), "A");
    assert_eq!(rows[0][0].rowspan, 3);
    assert!(rows[1][0].covered);
    assert!(rows[2][0].covered);
    assert_eq!(Token::collect_all_text(&rows[2][1].content), "3");
}

#[test]
fn rowspan_binds_to_nearest_cell_above_not_topmost() {
    // The `^` continues the cell directly above (B), not the one two
    // rows up (A).
    let tokens =
        parse("| K | V |\n| --- | --- |\n| A | x |\n| B | y |\n| ^ | z |\n");
    let (_, _, rows) = first_table(&tokens);
    assert_eq!(rows[0][0].rowspan, 1, "A should not span");
    assert_eq!(Token::collect_all_text(&rows[1][0].content), "B");
    assert_eq!(rows[1][0].rowspan, 2, "B continues into the ^ row");
    assert!(rows[2][0].covered);
}

#[test]
fn colspan_and_rowspan_combine_without_misbinding() {
    // Header spans cols 0..2; the `^` in the second body row sits in
    // physical column 0, which must resolve to the col-spanning origin
    // above it (the regression the logical-column walk got wrong).
    let tokens = parse(
        "| Span | > | Tail |\n| --- | --- | --- |\n| Merged | > | a |\n| ^ | > | b |\n",
    );
    let (headers, _, rows) = first_table(&tokens);
    assert_eq!(headers[0].colspan, 2);
    assert!(headers[1].covered);
    // Row 0: "Merged" colspan 2, then a covered slot, then "a".
    assert_eq!(Token::collect_all_text(&rows[0][0].content), "Merged");
    assert_eq!(rows[0][0].colspan, 2);
    assert_eq!(rows[0][0].rowspan, 2, "Merged extends into the ^ row");
    assert!(rows[0][1].covered);
    assert_eq!(Token::collect_all_text(&rows[0][2].content), "a");
    // Row 1: the `^` and its trailing `>` are both covered; only "b"
    // remains as real content.
    assert!(rows[1][0].covered);
    assert!(rows[1][1].covered);
    assert_eq!(Token::collect_all_text(&rows[1][2].content), "b");
}

#[test]
fn escaped_gt_is_literal_not_a_colspan_marker() {
    let tokens =
        parse("| a | \\> | c |\n| --- | --- | --- |\n| 1 | 2 | 3 |\n");
    let (headers, _, _) = first_table(&tokens);
    assert_eq!(headers.len(), 3);
    assert_eq!(headers[0].colspan, 1, "escaped marker must not extend");
    assert!(!headers[1].covered);
    assert_eq!(Token::collect_all_text(&headers[1].content), ">");
}

#[test]
fn escaped_caret_is_literal_not_a_rowspan_marker() {
    let tokens =
        parse("| K | V |\n| --- | --- |\n| A | one |\n| \\^ | two |\n");
    let (_, _, rows) = first_table(&tokens);
    assert_eq!(rows[0][0].rowspan, 1, "escaped marker must not extend");
    assert!(!rows[1][0].covered);
    assert_eq!(Token::collect_all_text(&rows[1][0].content), "^");
}

#[test]
fn leading_marker_with_no_origin_stays_literal() {
    let tokens = parse("| > | a | b |\n| --- | --- | --- |\n| ^ | x | y |\n");
    let (headers, _, rows) = first_table(&tokens);
    assert!(!headers[0].covered);
    assert_eq!(Token::collect_all_text(&headers[0].content), ">");
    assert_eq!(headers[0].colspan, 1);
    // A `^` in column 0 has no real cell above it (the header `>` is
    // literal but headers are not a rowspan source), so it stays
    // literal too rather than panicking or vanishing.
    assert!(!rows[0][0].covered);
    assert_eq!(Token::collect_all_text(&rows[0][0].content), "^");
}

#[test]
fn colspan_in_a_data_row() {
    let tokens = parse("| a | b | c |\n| --- | --- | --- |\n| wide | > | end |\n");
    let (_, _, rows) = first_table(&tokens);
    assert_eq!(Token::collect_all_text(&rows[0][0].content), "wide");
    assert_eq!(rows[0][0].colspan, 2);
    assert!(rows[0][1].covered);
    assert_eq!(Token::collect_all_text(&rows[0][2].content), "end");
}

/// CommonMark §4: block constructs may begin after 0-3 leading
/// spaces. Tables previously used a strict column-0 check and were
/// rejected at 1-3 spaces, dumping them back into paragraph text.
#[test]
fn table_with_three_leading_spaces_is_a_table() {
    let tokens = parse("   | a | b |\n   | --- | --- |\n   | 1 | 2 |\n");
    assert!(
        tokens.iter().any(|t| matches!(t, Token::Table { .. })),
        "table with 3-space leading indent should tokenize as Table"
    );
}

#[test]
fn table_with_four_leading_spaces_is_not_a_table() {
    // 4 cols crosses into indented-code territory; not a table per
    // the same 0-3 rule used by every other block marker.
    let tokens = parse("    | a | b |\n    | --- | --- |\n    | 1 | 2 |\n");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Table { .. })));
}

/// A GFM table inside a list-item body's blank-line continuation
/// must be tokenized as `Token::Table`, not consumed as literal pipe
/// text. The sub-lexer strips `content_offset` cols and the residual
/// 1-col indent must not block table dispatch.
#[test]
fn table_inside_list_item_body() {
    let md = "1. First item with a table:\n\n    | a | b |\n    | --- | --- |\n    | 1 | 2 |\n";
    let tokens = parse(md);
    let item = tokens
        .iter()
        .find(|t| matches!(t, Token::ListItem { .. }))
        .expect("list item not produced");
    let Token::ListItem { content, .. } = item else {
        unreachable!()
    };
    let table = content
        .iter()
        .find(|t| matches!(t, Token::Table { .. }))
        .expect("table not produced inside list item — fell back to literal pipes");
    let Token::Table { headers, rows, .. } = table else {
        unreachable!()
    };
    assert_eq!(headers.len(), 2);
    assert_eq!(rows.len(), 1);
}

/// Same for an unordered marker with the typical 2-col content
/// offset, where the residual indent after stripping is 2 cols.
#[test]
fn table_inside_unordered_list_item_body() {
    let md = "- bullet with table:\n\n  | a | b |\n  | --- | --- |\n  | x | y |\n";
    let tokens = parse(md);
    let item = tokens
        .iter()
        .find(|t| matches!(t, Token::ListItem { .. }))
        .expect("list item not produced");
    let Token::ListItem { content, .. } = item else {
        unreachable!()
    };
    assert!(
        content.iter().any(|t| matches!(t, Token::Table { .. })),
        "table inside unordered list item must tokenize as Table"
    );
}
