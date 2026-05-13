//! Tab handling corner cases. Targets `strip_leading_cols`,
//! tab handling in `parse_indented_code_block`.

use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn single_tab_is_4_columns_for_indented_code() {
    let tokens = parse("\tcode\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::Code { block: true, .. })));
}

#[test]
fn mixed_tabs_and_spaces_reach_4_cols() {
    // 2 spaces + 1 tab = 4 cols → indented code.
    let tokens = parse("  \tcode\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::Code { block: true, .. })));
}

#[test]
fn tab_inside_fenced_code_preserved() {
    let tokens = parse("```\nbody\twith\ttabs\n```");
    let Some(Token::Code { content, .. }) = tokens.iter().find(|t| matches!(t, Token::Code { block: true, .. })) else {
        panic!("expected fenced Code, got {:?}", tokens);
    };
    assert!(content.contains('\t'));
}

#[test]
fn tab_in_table_cell_works() {
    let tokens = parse("| a\tb | c |\n| --- | --- |\n| 1 | 2 |\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::Table { .. })));
}

#[test]
fn tab_between_atx_hashes_and_content_treated_as_space() {
    let tokens = parse("#\tHeading\n");
    let Some(Token::Heading(body, _)) = tokens.first() else {
        panic!("expected Heading, got {:?}", tokens);
    };
    assert_eq!(Token::collect_all_text(body).trim(), "Heading");
}

#[test]
fn tab_after_list_marker_creates_item() {
    let tokens = parse("-\titem\n");
    assert!(tokens.iter().any(|t| matches!(t, Token::ListItem { .. })));
}
